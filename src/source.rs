use crate::{
    collector::{command::SourceCommand, RpcClient, RpcResponseStats},
    config::{Config, ConnectorConfig},
    mode::{CommandId, ModeId, ProviderFeature, SolanaRpcCompatibility, Transport, COMMAND_COUNT},
    websocket::WebsocketProbeClient,
};
use anyhow::{bail, Context, Result};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tracing::warn;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSelection {
    pub mode: ModeId,
    pub command: CommandId,
    pub connector: Arc<str>,
    pub provider: Arc<str>,
    pub transport: Transport,
    pub solana_rpc_compatibility: Option<SolanaRpcCompatibility>,
    pub feature: Option<ProviderFeature>,
    pub precedence: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sourced<T> {
    pub value: T,
    pub source: SourceSelection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceRoute<'a> {
    pub mode: ModeId,
    pub command: CommandId,
    pub connector: &'a str,
    pub provider: &'a str,
    pub transport: Transport,
    pub solana_rpc_compatibility: Option<SolanaRpcCompatibility>,
    pub feature: Option<ProviderFeature>,
    pub precedence: usize,
    pub available: bool,
}

struct Connector {
    name: Arc<str>,
    provider: Arc<str>,
    solana_rpc_compatibility: Option<SolanaRpcCompatibility>,
    features: Vec<ProviderFeature>,
    rpc_client: Option<RpcClient>,
    ws_client: Option<WebsocketProbeClient>,
}

impl Connector {
    fn from_config(config: &ConnectorConfig, responses: RpcResponseStats) -> Result<Self> {
        let rpc_client = config
            .rpc_url
            .as_ref()
            .map(|url| {
                RpcClient::new(
                    url.clone(),
                    Duration::from_secs(config.rpc_timeout_sec),
                    config.commitment_config()?,
                    responses,
                )
            })
            .transpose()
            .with_context(|| {
                format!("failed to build RPC client for connector {:?}", config.name)
            })?;
        let ws_client = config.ws_url.as_ref().map(|url| {
            WebsocketProbeClient::new(url.clone(), Duration::from_secs(config.ws_timeout_sec))
        });

        Ok(Self {
            name: Arc::from(config.name.as_str()),
            provider: Arc::from(config.provider.as_str()),
            solana_rpc_compatibility: config.solana_rpc_compatibility,
            features: config.features.clone(),
            rpc_client,
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
            Transport::Rpc => self.rpc_client.is_some(),
            Transport::Websocket => self.ws_client.is_some(),
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

pub struct SourceChain {
    connectors: Vec<Connector>,
    routes: [Vec<usize>; COMMAND_COUNT],
}

impl SourceChain {
    pub fn from_config(config: &Config, responses: RpcResponseStats) -> Result<Self> {
        let connectors = config
            .connectors
            .iter()
            .map(|connector| Connector::from_config(connector, responses.clone()))
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

    pub async fn verify_capabilities(&self) -> Result<()> {
        for connector in &self.connectors {
            let (Some(client), Some(solana_rpc_compatibility)) = (
                connector.rpc_client.as_ref(),
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

    pub async fn required<C>(&self, command: &C) -> Result<Sourced<C::Output>>
    where
        C: SourceCommand,
    {
        let mut last_error = None;
        for (index, connector_index) in self.routes[C::ID.index()].iter().enumerate() {
            let connector = &self.connectors[*connector_index];
            let Some(client) = connector.rpc_client.as_ref() else {
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
                    last_error = Some((connector.name.as_ref(), error));
                }
            }
        }

        if let Some((connector, error)) = last_error {
            return Err(error).with_context(|| {
                format!(
                    "all connectors failed for required command {}; last connector was {}",
                    C::ID.as_str(),
                    connector
                )
            });
        }
        bail!(
            "no available connector for required command {}",
            C::ID.as_str()
        )
    }

    pub async fn optional<C>(&self, command: &C) -> Option<Sourced<C::Output>>
    where
        C: SourceCommand,
    {
        for (index, connector_index) in self.routes[C::ID.index()].iter().enumerate() {
            let connector = &self.connectors[*connector_index];
            let Some(client) = connector.rpc_client.as_ref() else {
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

    pub async fn websocket_healthy(&self) -> Result<Option<SourceSelection>> {
        let command = CommandId::WsHealth;
        let mut last_error = None;
        for (index, connector_index) in self.routes[command.index()].iter().enumerate() {
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
                    last_error = Some((connector.name.as_ref(), error));
                }
            }
        }

        match last_error {
            Some((connector, error)) => Err(error).with_context(|| {
                format!(
                    "all connectors failed for websocket health command; last connector was {}",
                    connector
                )
            }),
            None => Ok(None),
        }
    }
}
