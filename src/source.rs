//! Connector construction, command routing, fallback, and source provenance.
//!
//! A source chain compiles validated configuration into immutable per-command
//! routes and returns the connector that supplied each successful value.

use crate::{
    collector::{
        command::{CommandClients, CommandTransport, SourceCommand},
        JitoClient, RpcClient,
    },
    config::{Config, ConnectorConfig},
    mode::{CommandId, ModeId, ProviderFeature, SolanaRpcCompatibility, Transport, COMMAND_COUNT},
    watcher_state::RpcTelemetry,
    websocket::WebsocketProbeClient,
};
use anyhow::{bail, Context, Result};
use std::{collections::HashMap, fmt, sync::Arc};
use tracing::warn;

/// Provenance for the connector that successfully supplied one command result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSelection {
    /// Mode that owns the command.
    pub mode: ModeId,
    /// Command that produced the value.
    pub command: CommandId,
    /// Stable configured connector name.
    pub connector: Arc<str>,
    /// Operator-declared provider name.
    pub provider: Arc<str>,
    /// Transport used by the command.
    pub transport: Transport,
    /// Standard Solana RPC profile declared by the connector.
    pub solana_rpc_compatibility: Option<SolanaRpcCompatibility>,
    /// Provider feature required by the command, if any.
    pub feature: Option<ProviderFeature>,
    /// One-based position of the connector in the command's route.
    pub precedence: usize,
}

/// A collected value paired with its successful source.
#[derive(Clone, Debug, PartialEq)]
pub struct Sourced<T> {
    /// Command output.
    pub value: T,
    /// Provenance for the successful command attempt.
    pub source: SourceSelection,
}

/// Read-only description of one configured command-route entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceRoute<'a> {
    /// Mode that owns the command.
    pub mode: ModeId,
    /// Routed command.
    pub command: CommandId,
    /// Stable configured connector name.
    pub connector: &'a str,
    /// Operator-declared provider name.
    pub provider: &'a str,
    /// Required command transport.
    pub transport: Transport,
    /// Solana RPC profile declared by the connector.
    pub solana_rpc_compatibility: Option<SolanaRpcCompatibility>,
    /// Provider feature required by the command, if any.
    pub feature: Option<ProviderFeature>,
    /// One-based fallback position.
    pub precedence: usize,
    /// Whether the connector satisfies the command's declared requirements.
    pub available: bool,
}

#[derive(Debug)]
struct ConnectorAttemptFailure {
    connector: Arc<str>,
    provider: Arc<str>,
    error: anyhow::Error,
}

impl fmt::Display for ConnectorAttemptFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "connector {:?} (provider {:?}): {:#}",
            self.connector, self.provider, self.error
        )
    }
}

#[derive(Clone, Copy, Debug)]
enum RouteOperation {
    RequiredCommand(CommandId),
    WebsocketHealth,
}

impl fmt::Display for RouteOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequiredCommand(command) => {
                write!(formatter, "required command {}", command.as_str())
            }
            Self::WebsocketHealth => formatter.write_str("websocket health command"),
        }
    }
}

#[derive(Debug)]
struct RouteExhaustedError {
    operation: RouteOperation,
    failures: Vec<ConnectorAttemptFailure>,
}

impl fmt::Display for RouteExhaustedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "connector route exhausted for {}",
            self.operation
        )?;
        for failure in &self.failures {
            write!(formatter, "; {failure}")?;
        }
        Ok(())
    }
}

impl std::error::Error for RouteExhaustedError {}

struct Connector {
    name: Arc<str>,
    provider: Arc<str>,
    solana_rpc_compatibility: Option<SolanaRpcCompatibility>,
    features: Vec<ProviderFeature>,
    command_clients: CommandClients,
    ws_client: Option<WebsocketProbeClient>,
}

impl Connector {
    fn from_config(config: &ConnectorConfig, telemetry: RpcTelemetry) -> Result<Self> {
        let rpc_client = config
            .rpc
            .as_ref()
            .map(|transport| RpcClient::try_new(transport, telemetry.for_connector(&config.name)))
            .transpose()
            .with_context(|| {
                format!("failed to build RPC client for connector {:?}", config.name)
            })?;
        let jito_client = config
            .provider_api
            .as_ref()
            .map(JitoClient::try_new)
            .transpose()
            .with_context(|| {
                format!(
                    "failed to build provider API client for connector {:?}",
                    config.name
                )
            })?;
        let ws_client = config
            .websocket
            .as_ref()
            .map(|transport| {
                WebsocketProbeClient::try_new(
                    transport,
                    telemetry.for_websocket_connector(&config.name),
                )
            })
            .transpose()
            .with_context(|| {
                format!(
                    "failed to build WebSocket client for connector {:?}",
                    config.name
                )
            })?;

        Ok(Self {
            name: Arc::from(config.name.as_str()),
            provider: Arc::from(config.provider.as_str()),
            solana_rpc_compatibility: config.solana_rpc_compatibility,
            features: config.features.clone(),
            command_clients: CommandClients::new(rpc_client, jito_client),
            ws_client,
        })
    }

    fn selection(&self, command: CommandId, precedence: usize) -> SourceSelection {
        let requirement = command.source_requirement();
        SourceSelection {
            mode: command.mode(),
            command,
            connector: Arc::clone(&self.name),
            provider: Arc::clone(&self.provider),
            transport: requirement.transport,
            solana_rpc_compatibility: self.solana_rpc_compatibility,
            feature: requirement.feature,
            precedence,
        }
    }

    fn supports_requirement(&self, command: CommandId) -> bool {
        let requirement = command.source_requirement();
        let transport_available = match requirement.transport {
            Transport::Rpc => self.command_clients.rpc().is_some(),
            Transport::Websocket => self.ws_client.is_some(),
            Transport::ProviderApi => self.command_clients.jito_api().is_some(),
        };
        let solana_rpc_compatible = requirement.solana_rpc_compatibility.is_none_or(|required| {
            self.solana_rpc_compatibility
                .is_some_and(|declared| declared.supports(required))
        });
        let feature_available = requirement
            .feature
            .is_none_or(|feature| self.features.contains(&feature));

        transport_available && solana_rpc_compatible && feature_available
    }
}

/// Immutable connector clients and compiled fallback routes for every command.
pub struct SourceChain {
    connectors: Vec<Connector>,
    routes: [Vec<usize>; COMMAND_COUNT],
}

impl SourceChain {
    /// Builds clients once and compiles validated connector names into route indices.
    pub fn from_config(config: &Config, telemetry: RpcTelemetry) -> Result<Self> {
        let connectors = config
            .connectors
            .iter()
            .map(|connector| Connector::from_config(connector, telemetry.clone()))
            .collect::<Result<Vec<_>>>()?;
        let connector_indices = connectors
            .iter()
            .enumerate()
            .map(|(index, connector)| (connector.name.as_ref(), index))
            .collect::<HashMap<_, _>>();
        let mut routes: [Vec<usize>; COMMAND_COUNT] = std::array::from_fn(|_| Vec::new());

        for command in CommandId::ALL {
            let route = &mut routes[command.index()];
            route.reserve(config.modes.route(command).len());
            for connector_name in config.modes.route(command) {
                let connector_index = connector_indices
                    .get(connector_name.as_str())
                    .copied()
                    .with_context(|| {
                        format!(
                            "command {} references unknown connector {:?}",
                            command.as_str(),
                            connector_name
                        )
                    })?;
                route.push(connector_index);
            }
        }

        Ok(Self { connectors, routes })
    }

    /// Iterates every declared route entry in command and precedence order.
    pub fn routes(&self) -> impl Iterator<Item = SourceRoute<'_>> + '_ {
        CommandId::ALL.into_iter().flat_map(move |command| {
            self.routes[command.index()]
                .iter()
                .enumerate()
                .map(move |(index, connector_index)| {
                    let connector = &self.connectors[*connector_index];
                    let requirement = command.source_requirement();
                    SourceRoute {
                        mode: command.mode(),
                        command,
                        connector: connector.name.as_ref(),
                        provider: connector.provider.as_ref(),
                        transport: requirement.transport,
                        solana_rpc_compatibility: connector.solana_rpc_compatibility,
                        feature: requirement.feature,
                        precedence: index + 1,
                        available: connector.supports_requirement(command),
                    }
                })
        })
    }

    /// Probes every connector method promised by its declared capabilities.
    ///
    /// The first failing connector aborts verification with connector and provider
    /// context. Endpoint URLs are not included in that context.
    pub async fn verify_capabilities(&self) -> Result<()> {
        for connector in &self.connectors {
            let (Some(client), Some(solana_rpc_compatibility)) = (
                connector.command_clients.rpc(),
                connector.solana_rpc_compatibility,
            ) else {
                continue;
            };
            client
                .verify_capabilities(solana_rpc_compatibility, &connector.features)
                .await
                .with_context(|| {
                    format!(
                        "connector {:?} provider {:?} RPC endpoint capability verification failed",
                        connector.name, connector.provider
                    )
                })?;
        }

        Ok(())
    }

    /// Executes a required command against its route until one connector succeeds.
    ///
    /// Returns every causal connector error when all declared candidates fail.
    pub async fn required<C>(&self, command: &C) -> Result<Sourced<C::Output>>
    where
        C: SourceCommand,
    {
        let route = &self.routes[C::ID.index()];
        let mut failures = Vec::new();
        for (index, connector_index) in route.iter().enumerate() {
            let connector = &self.connectors[*connector_index];
            let Some(client) = C::Transport::client(&connector.command_clients) else {
                continue;
            };
            match command.execute(client).await {
                Ok(value) => {
                    return Ok(Sourced {
                        value,
                        source: connector.selection(C::ID, index + 1),
                    });
                }
                Err(error) => {
                    warn!(
                        mode = C::ID.mode().as_str(),
                        command = C::ID.as_str(),
                        connector = connector.name.as_ref(),
                        provider = connector.provider.as_ref(),
                        error = ?error,
                        "connector failed for required command; trying next connector"
                    );
                    failures.push(ConnectorAttemptFailure {
                        connector: Arc::clone(&connector.name),
                        provider: Arc::clone(&connector.provider),
                        error,
                    });
                }
            }
        }

        if !failures.is_empty() {
            return Err(RouteExhaustedError {
                operation: RouteOperation::RequiredCommand(C::ID),
                failures,
            }
            .into());
        }
        bail!(
            "no available connector for required command {}",
            C::ID.as_str()
        )
    }

    /// Executes an optional command against its route until one connector succeeds.
    ///
    /// Every failed attempt remains observable, but route exhaustion returns
    /// `None` rather than failing the polling round.
    pub async fn optional<C>(&self, command: &C) -> Option<Sourced<C::Output>>
    where
        C: SourceCommand,
    {
        for (index, connector_index) in self.routes[C::ID.index()].iter().enumerate() {
            let connector = &self.connectors[*connector_index];
            let Some(client) = C::Transport::client(&connector.command_clients) else {
                continue;
            };
            match command.execute(client).await {
                Ok(value) => {
                    return Some(Sourced {
                        value,
                        source: connector.selection(C::ID, index + 1),
                    });
                }
                Err(error) => warn!(
                    mode = C::ID.mode().as_str(),
                    command = C::ID.as_str(),
                    connector = connector.name.as_ref(),
                    provider = connector.provider.as_ref(),
                    error = ?error,
                    "connector failed for optional command; trying next connector"
                ),
            }
        }

        None
    }

    /// Probes the configured WebSocket health route with ordered fallback.
    ///
    /// Every failed connector attempt records telemetry and remains in the returned
    /// error when the route is exhausted. Returns `Ok(None)` when no WebSocket route
    /// is configured.
    pub async fn websocket_healthy(&self) -> Result<Option<SourceSelection>> {
        let command = CommandId::WsHealth;
        let route = &self.routes[command.index()];
        let mut failures = Vec::new();
        for (index, connector_index) in route.iter().enumerate() {
            let connector = &self.connectors[*connector_index];
            let Some(client) = connector.ws_client.as_ref() else {
                continue;
            };
            match client.probe().await {
                Ok(()) => return Ok(Some(connector.selection(command, index + 1))),
                Err(error) => {
                    warn!(
                        mode = command.mode().as_str(),
                        command = command.as_str(),
                        connector = connector.name.as_ref(),
                        provider = connector.provider.as_ref(),
                        error = ?error,
                        "connector failed for websocket health command; trying next connector"
                    );
                    failures.push(ConnectorAttemptFailure {
                        connector: Arc::clone(&connector.name),
                        provider: Arc::clone(&connector.provider),
                        error,
                    });
                }
            }
        }

        if failures.is_empty() {
            Ok(None)
        } else {
            Err(RouteExhaustedError {
                operation: RouteOperation::WebsocketHealth,
                failures,
            }
            .into())
        }
    }
}

#[cfg(test)]
mod tests;
