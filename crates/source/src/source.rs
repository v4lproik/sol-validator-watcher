//! Connector construction, command routing, fallback, and source provenance.
//!
//! A source chain compiles validated configuration into immutable per-command
//! routes and returns the connector that supplied each successful value.

pub(crate) use crate::mode::SourceSelection;
use crate::{
    collector::{
        command::{
            CommandClients, CommandTransport, GetHealth, GetRecentPerformanceSamples,
            GetVoteAccounts, SourceCommand,
        },
        ClusterPerformanceMetrics, JitoClient, NodeHealth, OperatingCostsClient, RpcClient,
        TrilliumClient,
    },
    config::{Config, ConnectorConfig},
    failover::{
        classify_failure, CircuitTransitions, FailoverAttempt, FailoverAttemptDecision,
        FailoverController, FailoverGateIndices, FailoverKey, FailoverScope, FailoverTransition,
    },
    mode::{
        CommandId, ModeId, ProviderFeature, RuntimeConnectorInventory, RuntimeInventory,
        RuntimeModeInventory, RuntimeRouteInventory, SolanaRpcCompatibility, Transport,
        COMMAND_COUNT,
    },
    telemetry::RuntimeTelemetry,
    websocket::WebsocketProbeClient,
};
use anyhow::{anyhow, bail, Context, Result};
use std::{
    collections::{BTreeSet, HashMap},
    fmt,
    sync::Arc,
    time::Duration,
};
use tracing::{info, warn};

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

const FAILOVER_COOLDOWN: Duration = Duration::from_secs(60);

#[derive(Clone, Copy)]
enum RoutePolicy {
    Required,
    Optional,
    WebsocketHealth,
}

impl RoutePolicy {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Optional => "optional",
            Self::WebsocketHealth => "websocket-health",
        }
    }
}

#[derive(Clone, Copy)]
struct CompiledRouteEntry {
    connector_index: usize,
    failover_gates: Option<FailoverGateIndices>,
}

struct Connector {
    name: Arc<str>,
    provider: Arc<str>,
    solana_rpc_compatibility: Option<SolanaRpcCompatibility>,
    features: Vec<ProviderFeature>,
    command_clients: CommandClients,
    ws_client: Option<WebsocketProbeClient>,
}

impl Connector {
    fn from_config(config: &ConnectorConfig, telemetry: &RuntimeTelemetry) -> Result<Self> {
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
            .filter(|_| config.supports(ProviderFeature::JitoMevRewards))
            .map(JitoClient::try_new)
            .transpose()
            .with_context(|| {
                format!(
                    "failed to build Jito API client for connector {:?}",
                    config.name
                )
            })?;
        let trillium_client = config
            .provider_api
            .as_ref()
            .filter(|_| config.supports(ProviderFeature::TrilliumMevCapture))
            .map(TrilliumClient::try_new)
            .transpose()
            .with_context(|| {
                format!(
                    "failed to build Trillium API client for connector {:?}",
                    config.name
                )
            })?;
        let operating_costs_client = config
            .provider_api
            .as_ref()
            .filter(|_| config.supports(ProviderFeature::OperatingCosts))
            .map(OperatingCostsClient::try_new)
            .transpose()
            .with_context(|| {
                format!(
                    "failed to build operating-cost API client for connector {:?}",
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
            command_clients: CommandClients::new(
                rpc_client,
                jito_client,
                trillium_client,
                operating_costs_client,
            ),
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
            transport: requirement.transport(),
            solana_rpc_compatibility: self.solana_rpc_compatibility,
            feature: requirement.feature(),
            precedence,
        }
    }

    fn supports_requirement(&self, command: CommandId) -> bool {
        let requirement = command.source_requirement();
        let transport_available = match requirement.transport() {
            Transport::Rpc => self.command_clients.rpc().is_some(),
            Transport::Websocket => self.ws_client.is_some(),
            Transport::ProviderApi => match requirement.feature() {
                Some(ProviderFeature::JitoMevRewards) => self.command_clients.jito_api().is_some(),
                Some(ProviderFeature::TrilliumMevCapture) => {
                    self.command_clients.trillium_api().is_some()
                }
                Some(ProviderFeature::OperatingCosts) => {
                    self.command_clients.operating_costs_api().is_some()
                }
                _ => false,
            },
        };
        let solana_rpc_compatible = requirement
            .solana_rpc_compatibility()
            .is_none_or(|required| {
                self.solana_rpc_compatibility
                    .is_some_and(|declared| declared.supports(required))
            });
        let feature_available = requirement
            .feature()
            .is_none_or(|feature| self.features.contains(&feature));

        transport_available && solana_rpc_compatible && feature_available
    }
}

/// Connector clients, immutable command routes, and hierarchical failover state.
pub struct SourceChain {
    connectors: Vec<Connector>,
    routes: [Vec<CompiledRouteEntry>; COMMAND_COUNT],
    failovers: FailoverController,
    failover_inventory: Vec<FailoverKey>,
    telemetry: RuntimeTelemetry,
}

impl SourceChain {
    /// Builds clients once and compiles validated connector names into route indices.
    pub fn from_config(config: &Config, telemetry: RuntimeTelemetry) -> Result<Self> {
        let connectors = config
            .connectors
            .iter()
            .map(|connector| Connector::from_config(connector, &telemetry))
            .collect::<Result<Vec<_>>>()?;
        let connector_indices = connectors
            .iter()
            .enumerate()
            .map(|(index, connector)| (connector.name.as_ref(), index))
            .collect::<HashMap<_, _>>();
        let mut source_requirement_gate_indices = HashMap::new();
        let mut command_gate_indices = HashMap::new();
        let mut failover_inventory = BTreeSet::new();
        let mut routes: [Vec<CompiledRouteEntry>; COMMAND_COUNT] =
            std::array::from_fn(|_| Vec::new());

        for command in CommandId::ALL {
            let requirement = command.source_requirement();
            let configured_route = config.modes.route(command);
            let route = &mut routes[command.index()];
            route.reserve(configured_route.len());
            let mut offset = 0;
            while offset < configured_route.len() {
                let connector_name = &configured_route[offset];
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
                let backups = config.connectors[connector_index].backups.get(&requirement);
                let failover_gates = backups.map(|_| {
                    let next_source_requirement_index = source_requirement_gate_indices.len();
                    let source_requirement = *source_requirement_gate_indices
                        .entry((connector_index, requirement))
                        .or_insert(next_source_requirement_index);
                    let next_command_index = command_gate_indices.len();
                    let command_gate = *command_gate_indices
                        .entry((connector_index, command))
                        .or_insert(next_command_index);
                    let connector = &connectors[connector_index];
                    failover_inventory.insert(FailoverKey {
                        connector: Arc::clone(&connector.name),
                        provider: Arc::clone(&connector.provider),
                        source_requirement: requirement,
                        scope: FailoverScope::SourceRequirement,
                        command: None,
                    });
                    failover_inventory.insert(FailoverKey {
                        connector: Arc::clone(&connector.name),
                        provider: Arc::clone(&connector.provider),
                        source_requirement: requirement,
                        scope: FailoverScope::Command,
                        command: Some(command),
                    });
                    FailoverGateIndices {
                        source_requirement,
                        command: command_gate,
                    }
                });
                route.push(CompiledRouteEntry {
                    connector_index,
                    failover_gates,
                });
                offset += 1;

                if let Some(backups) = backups {
                    for backup_name in backups {
                        let expanded_name = configured_route.get(offset).with_context(|| {
                            format!(
                                "command {} route omits configured backup {:?} for connector {:?}",
                                command.as_str(),
                                backup_name,
                                connector_name
                            )
                        })?;
                        if expanded_name != backup_name {
                            bail!(
                                "command {} route expected backup {:?} for connector {:?}, found {:?}",
                                command.as_str(),
                                backup_name,
                                connector_name,
                                expanded_name
                            );
                        }
                        let backup_index = connector_indices
                            .get(backup_name.as_str())
                            .copied()
                            .with_context(|| {
                                format!(
                                    "command {} references unknown backup connector {:?}",
                                    command.as_str(),
                                    backup_name
                                )
                            })?;
                        route.push(CompiledRouteEntry {
                            connector_index: backup_index,
                            failover_gates: None,
                        });
                        offset += 1;
                    }
                }
            }
        }

        let failovers = FailoverController::new(
            source_requirement_gate_indices.len(),
            command_gate_indices.len(),
            FAILOVER_COOLDOWN,
        );
        Ok(Self {
            connectors,
            routes,
            failovers,
            failover_inventory: failover_inventory.into_iter().collect(),
            telemetry,
        })
    }

    #[cfg(test)]
    pub(crate) fn with_failover_cooldown(mut self, cooldown: Duration) -> Self {
        self.failovers = self.failovers.with_cooldown(cooldown);
        self
    }

    /// Returns every configured failover circuit in deterministic order.
    pub fn failover_inventory(&self) -> &[FailoverKey] {
        &self.failover_inventory
    }

    /// Executes the required RPC health command through its configured source route.
    pub async fn rpc_health(&self) -> Result<Sourced<NodeHealth>> {
        self.required(&GetHealth).await
    }

    /// Executes the optional recent-performance command through its configured route.
    pub async fn recent_performance(
        &self,
    ) -> Result<Option<Sourced<Option<ClusterPerformanceMetrics>>>> {
        self.optional(&GetRecentPerformanceSamples).await
    }

    /// Executes the required vote-accounts command and returns its successful source.
    pub async fn vote_accounts_source(
        &self,
        delinquent_slot_distance: Option<u64>,
    ) -> Result<SourceSelection> {
        Ok(self
            .required(&GetVoteAccounts {
                delinquent_slot_distance,
            })
            .await?
            .source)
    }

    /// Iterates every declared route entry in command and precedence order.
    pub fn routes(&self) -> impl Iterator<Item = SourceRoute<'_>> + '_ {
        CommandId::ALL.into_iter().flat_map(move |command| {
            self.routes[command.index()]
                .iter()
                .enumerate()
                .map(move |(index, route_entry)| {
                    let connector = &self.connectors[route_entry.connector_index];
                    let requirement = command.source_requirement();
                    SourceRoute {
                        mode: command.mode(),
                        command,
                        connector: connector.name.as_ref(),
                        provider: connector.provider.as_ref(),
                        transport: requirement.transport(),
                        solana_rpc_compatibility: connector.solana_rpc_compatibility,
                        feature: requirement.feature(),
                        precedence: index + 1,
                        available: connector.supports_requirement(command),
                    }
                })
        })
    }

    /// Builds bounded, non-secret runtime metadata for metrics publication.
    pub fn runtime_inventory(&self, config: &Config) -> RuntimeInventory {
        let connectors = config
            .connectors
            .iter()
            .enumerate()
            .map(|(index, connector)| RuntimeConnectorInventory {
                name: Arc::from(connector.name.as_str()),
                provider: Arc::from(connector.provider.as_str()),
                declaration_order: index + 1,
                solana_rpc_compatibility: connector.solana_rpc_compatibility,
                has_rpc: connector.has_rpc(),
                has_websocket: connector.has_ws(),
                has_provider_api: connector.has_provider_api(),
                features: connector.features.clone(),
            })
            .collect();
        let modes = ModeId::ALL
            .into_iter()
            .map(|mode_id| RuntimeModeInventory {
                mode_id,
                enabled: config.modes.enabled(mode_id),
            })
            .collect();
        let routes = self
            .routes()
            .map(|route| RuntimeRouteInventory {
                mode: route.mode,
                command: route.command,
                connector: Arc::from(route.connector),
                provider: Arc::from(route.provider),
                transport: route.transport,
                solana_rpc_compatibility: route.solana_rpc_compatibility,
                feature: route.feature,
                precedence: route.precedence,
                available: route.available,
            })
            .collect();
        RuntimeInventory {
            network: Arc::from(config.network.as_str()),
            config_fingerprint: Arc::from(config.fingerprint()),
            poll_interval_sec: config.poll_interval_sec,
            websocket_configured: config.connectors.iter().any(ConnectorConfig::has_ws),
            connectors,
            modes,
            routes,
        }
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

    async fn begin_route_attempt(
        &self,
        entry: CompiledRouteEntry,
        connector: &Connector,
        command: CommandId,
    ) -> Result<FailoverAttemptDecision> {
        let Some(gates) = entry.failover_gates else {
            return Ok(FailoverAttemptDecision::Attempt(FailoverAttempt::ungated()));
        };
        let (decision, transitions) = self.failovers.begin_attempt(gates).await;
        self.publish_failover_transitions(connector, command, transitions)?;
        if matches!(
            decision,
            FailoverAttemptDecision::Attempt(attempt) if attempt.is_recovery_probe()
        ) {
            info!(
                mode = command.mode().as_str(),
                command = command.as_str(),
                connector = connector.name.as_ref(),
                provider = connector.provider.as_ref(),
                source_requirement = command.source_requirement().as_str(),
                "probing primary connector after failover cooldown"
            );
        }
        Ok(decision)
    }

    fn publish_failover_transitions(
        &self,
        connector: &Connector,
        command: CommandId,
        transitions: CircuitTransitions,
    ) -> Result<()> {
        for transition in transitions.into_iter().flatten() {
            self.telemetry
                .record_failover_transition(FailoverTransition {
                    key: FailoverKey {
                        connector: Arc::clone(&connector.name),
                        provider: Arc::clone(&connector.provider),
                        source_requirement: command.source_requirement(),
                        scope: transition.scope,
                        command: (transition.scope == FailoverScope::Command).then_some(command),
                    },
                    status: transition.status,
                    kind: transition.kind,
                })
                .with_context(|| {
                    format!(
                        "failed to publish {} failover transition for connector {:?} command {:?}",
                        transition.scope.as_str(),
                        connector.name,
                        command.as_str()
                    )
                })?;
        }
        Ok(())
    }

    async fn record_missing_client(
        &self,
        entry: CompiledRouteEntry,
        connector: &Connector,
        command: CommandId,
        attempt: FailoverAttempt,
        policy: RoutePolicy,
    ) -> Result<anyhow::Error> {
        let error = anyhow!(
            "connector {:?} provider {:?} has no client for command {:?}",
            connector.name,
            connector.provider,
            command.as_str()
        );
        self.record_route_failure(entry, connector, command, attempt, policy, &error)
            .await?;
        Ok(error)
    }

    async fn record_route_success(
        &self,
        entry: CompiledRouteEntry,
        connector: &Connector,
        command: CommandId,
        attempt: FailoverAttempt,
    ) -> Result<()> {
        let Some(gates) = entry.failover_gates else {
            return Ok(());
        };
        let transitions = self.failovers.record_success(gates, attempt).await;
        self.publish_failover_transitions(connector, command, transitions)
    }

    async fn record_route_failure(
        &self,
        entry: CompiledRouteEntry,
        connector: &Connector,
        command: CommandId,
        attempt: FailoverAttempt,
        policy: RoutePolicy,
        error: &anyhow::Error,
    ) -> Result<()> {
        let failure_scope = classify_failure(error);
        let transitions = match entry.failover_gates {
            Some(gates) => {
                self.failovers
                    .record_failure(gates, attempt, failure_scope)
                    .await
            }
            None => [None, None],
        };
        let failover_transition_count = transitions.iter().flatten().count();
        self.publish_failover_transitions(connector, command, transitions)?;
        warn!(
            mode = command.mode().as_str(),
            command = command.as_str(),
            connector = connector.name.as_ref(),
            provider = connector.provider.as_ref(),
            source_requirement = command.source_requirement().as_str(),
            failure_scope = failure_scope.as_str(),
            route_policy = policy.as_str(),
            recovery_probe = attempt.is_recovery_probe(),
            failover_transition_count,
            error = ?error,
            "connector command attempt failed; advancing source route"
        );
        Ok(())
    }

    /// Executes a required command against its route until one connector succeeds.
    ///
    /// Returns every causal connector error when all declared candidates fail.
    pub(crate) async fn required<C>(&self, command: &C) -> Result<Sourced<C::Output>>
    where
        C: SourceCommand,
    {
        let route = &self.routes[C::ID.index()];
        let mut failures = Vec::new();
        for (index, entry) in route.iter().copied().enumerate() {
            let connector = &self.connectors[entry.connector_index];
            let FailoverAttemptDecision::Attempt(attempt) =
                self.begin_route_attempt(entry, connector, C::ID).await?
            else {
                continue;
            };
            let Some(client) = C::Transport::client(&connector.command_clients) else {
                let error = self
                    .record_missing_client(entry, connector, C::ID, attempt, RoutePolicy::Required)
                    .await?;
                failures.push(ConnectorAttemptFailure {
                    connector: Arc::clone(&connector.name),
                    provider: Arc::clone(&connector.provider),
                    error,
                });
                continue;
            };
            match command.execute(client).await {
                Ok(value) => {
                    self.record_route_success(entry, connector, C::ID, attempt)
                        .await?;
                    return Ok(Sourced {
                        value,
                        source: connector.selection(C::ID, index + 1),
                    });
                }
                Err(error) => {
                    self.record_route_failure(
                        entry,
                        connector,
                        C::ID,
                        attempt,
                        RoutePolicy::Required,
                        &error,
                    )
                    .await?;
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
    /// `Ok(None)` rather than failing the polling round.
    pub(crate) async fn optional<C>(&self, command: &C) -> Result<Option<Sourced<C::Output>>>
    where
        C: SourceCommand,
    {
        for (index, entry) in self.routes[C::ID.index()].iter().copied().enumerate() {
            let connector = &self.connectors[entry.connector_index];
            let FailoverAttemptDecision::Attempt(attempt) =
                self.begin_route_attempt(entry, connector, C::ID).await?
            else {
                continue;
            };
            let Some(client) = C::Transport::client(&connector.command_clients) else {
                self.record_missing_client(entry, connector, C::ID, attempt, RoutePolicy::Optional)
                    .await?;
                continue;
            };
            match command.execute(client).await {
                Ok(value) => {
                    self.record_route_success(entry, connector, C::ID, attempt)
                        .await?;
                    return Ok(Some(Sourced {
                        value,
                        source: connector.selection(C::ID, index + 1),
                    }));
                }
                Err(error) => {
                    self.record_route_failure(
                        entry,
                        connector,
                        C::ID,
                        attempt,
                        RoutePolicy::Optional,
                        &error,
                    )
                    .await?;
                }
            }
        }

        Ok(None)
    }

    /// Probes the configured WebSocket health route with source failover.
    ///
    /// Every failed connector attempt records telemetry and remains in the returned
    /// error when the route is exhausted. Returns `Ok(None)` when no WebSocket route
    /// is configured.
    pub async fn websocket_healthy(&self) -> Result<Option<SourceSelection>> {
        let command = CommandId::WsHealth;
        let route = &self.routes[command.index()];
        let mut failures = Vec::new();
        for (index, entry) in route.iter().copied().enumerate() {
            let connector = &self.connectors[entry.connector_index];
            let FailoverAttemptDecision::Attempt(attempt) =
                self.begin_route_attempt(entry, connector, command).await?
            else {
                continue;
            };
            let Some(client) = connector.ws_client.as_ref() else {
                let error = self
                    .record_missing_client(
                        entry,
                        connector,
                        command,
                        attempt,
                        RoutePolicy::WebsocketHealth,
                    )
                    .await?;
                failures.push(ConnectorAttemptFailure {
                    connector: Arc::clone(&connector.name),
                    provider: Arc::clone(&connector.provider),
                    error,
                });
                continue;
            };
            match client.probe().await {
                Ok(()) => {
                    self.record_route_success(entry, connector, command, attempt)
                        .await?;
                    return Ok(Some(connector.selection(command, index + 1)));
                }
                Err(error) => {
                    self.record_route_failure(
                        entry,
                        connector,
                        command,
                        attempt,
                        RoutePolicy::WebsocketHealth,
                        &error,
                    )
                    .await?;
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
