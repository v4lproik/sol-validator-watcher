// `SourceCommand` must promise a `Send` future; trait `async fn` cannot express that bound.
#![allow(clippy::manual_async_fn)]

use crate::{
    collector::RpcClient,
    config::{Config, ConnectorKind},
    provider::{
        Capability, CapabilityPriorityOverrides, CommandId, CommandPriorityOverrides,
        ProviderDriver, COMMAND_COUNT,
    },
    websocket::WebsocketProbeClient,
};
use anyhow::{bail, Context, Result};
use std::{future::Future, sync::Arc, time::Duration};
use tracing::warn;

pub(crate) struct Connector {
    name: Arc<str>,
    kind: ConnectorKind,
    driver: ProviderDriver,
    priority: i32,
    capability_priorities: CapabilityPriorityOverrides,
    command_priorities: CommandPriorityOverrides,
    declaration_order: usize,
    rpc_client: Option<RpcClient>,
    websocket_client: Option<WebsocketProbeClient>,
}

impl Connector {
    fn from_config(config: &Config) -> Result<Vec<Self>> {
        config
            .effective_connectors()
            .into_iter()
            .enumerate()
            .map(|(declaration_order, connector)| {
                let rpc_client = match connector.rpc_url.as_ref() {
                    Some(rpc_url) => Some(RpcClient::new(
                        rpc_url.clone(),
                        Duration::from_secs(connector.rpc_timeout_sec),
                        connector.commitment_config()?,
                    )?),
                    None => None,
                };
                let websocket_client = if config.validator_websocket_enabled {
                    connector.ws_url.as_ref().map(|url| {
                        WebsocketProbeClient::new(
                            url.clone(),
                            Duration::from_secs(connector.ws_timeout_sec),
                        )
                    })
                } else {
                    None
                };

                Ok(Self {
                    name: Arc::from(connector.name),
                    kind: connector.kind,
                    driver: connector.kind.driver(),
                    priority: connector.priority,
                    capability_priorities: connector.capabilities,
                    command_priorities: connector.commands,
                    declaration_order,
                    rpc_client,
                    websocket_client,
                })
            })
            .collect()
    }

    pub(crate) fn rpc(&self) -> Result<&RpcClient> {
        self.rpc_client
            .as_ref()
            .with_context(|| format!("connector {} has no RPC client", self.name))
    }

    fn websocket(&self) -> Result<&WebsocketProbeClient> {
        self.websocket_client
            .as_ref()
            .with_context(|| format!("connector {} has no websocket client", self.name))
    }

    fn supports(&self, command: CommandId) -> bool {
        match command.capability() {
            Capability::HealthRpc => {
                self.rpc_client.is_some() && self.driver.supports(Capability::HealthRpc)
            }
            Capability::HealthWs => self.websocket_client.is_some(),
            capability => self.rpc_client.is_some() && self.driver.supports(capability),
        }
    }

    fn priority_for(&self, command: CommandId) -> i32 {
        self.command_priorities
            .get(&command)
            .or_else(|| self.capability_priorities.get(&command.capability()))
            .copied()
            .unwrap_or(self.priority)
    }

    fn selection(&self, command: CommandId) -> SourceSelection {
        SourceSelection {
            command,
            connector: Arc::clone(&self.name),
            kind: self.kind,
            driver: self.driver,
            effective_priority: self.priority_for(command),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSelection {
    pub command: CommandId,
    pub connector: Arc<str>,
    pub kind: ConnectorKind,
    pub driver: ProviderDriver,
    pub effective_priority: i32,
}

pub(crate) struct Sourced<T> {
    pub(crate) value: T,
    pub(crate) source: SourceSelection,
}

pub(crate) trait SourceCommand {
    type Output;

    const ID: CommandId;

    fn execute<'a>(
        &'a self,
        connector: &'a Connector,
    ) -> impl Future<Output = Result<Self::Output>> + Send + 'a;
}

enum ChainAttempt<T> {
    Resolved(Sourced<T>),
    Exhausted {
        last_connector: Option<usize>,
        last_error: Option<anyhow::Error>,
    },
}

pub struct SourceChain {
    connectors: Vec<Connector>,
    orderings: [Vec<usize>; COMMAND_COUNT],
}

impl SourceChain {
    pub fn from_config(config: &Config) -> Result<Self> {
        Ok(Self::new(Connector::from_config(config)?))
    }

    fn new(connectors: Vec<Connector>) -> Self {
        let orderings = std::array::from_fn(|index| {
            let command = CommandId::ALL[index];
            let mut ordering = connectors
                .iter()
                .enumerate()
                .filter_map(|(position, connector)| connector.supports(command).then_some(position))
                .collect::<Vec<_>>();
            ordering.sort_unstable_by(|left, right| {
                connectors[*right]
                    .priority_for(command)
                    .cmp(&connectors[*left].priority_for(command))
                    .then(
                        connectors[*left]
                            .declaration_order
                            .cmp(&connectors[*right].declaration_order),
                    )
            });
            ordering
        });

        Self {
            connectors,
            orderings,
        }
    }

    pub fn connector_count(&self) -> usize {
        self.connectors.len()
    }

    pub async fn ensure_rpc_healthy(&self) -> Result<()> {
        self.required(&RpcHealthCommand).await.map(|_| ())
    }

    pub async fn websocket_healthy(&self) -> bool {
        self.optional(&WebsocketHealthCommand).await.is_some()
    }

    pub(crate) async fn required<C>(&self, command: &C) -> Result<Sourced<C::Output>>
    where
        C: SourceCommand,
    {
        match self.try_sources(command).await {
            ChainAttempt::Resolved(resolved) => Ok(resolved),
            ChainAttempt::Exhausted {
                last_connector: Some(index),
                last_error: Some(error),
            } => Err(error).with_context(|| {
                format!(
                    "all connectors failed for required command {}; last connector was {}",
                    C::ID.as_str(),
                    self.connectors[index].name
                )
            }),
            ChainAttempt::Exhausted { .. } => {
                bail!("no connector supports required command {}", C::ID.as_str())
            }
        }
    }

    pub(crate) async fn optional<C>(&self, command: &C) -> Option<Sourced<C::Output>>
    where
        C: SourceCommand,
    {
        match self.try_sources(command).await {
            ChainAttempt::Resolved(resolved) => Some(resolved),
            ChainAttempt::Exhausted { .. } => None,
        }
    }

    async fn try_sources<C>(&self, command: &C) -> ChainAttempt<C::Output>
    where
        C: SourceCommand,
    {
        let mut last_connector = None;
        let mut last_error = None;

        for &index in &self.orderings[C::ID.index()] {
            let connector = &self.connectors[index];
            match command.execute(connector).await {
                Ok(value) => {
                    return ChainAttempt::Resolved(Sourced {
                        value,
                        source: connector.selection(C::ID),
                    });
                }
                Err(error) => {
                    warn!(
                        connector = connector.name.as_ref(),
                        connector_kind = connector.kind.as_str(),
                        provider_driver = connector.driver.as_str(),
                        command = C::ID.as_str(),
                        capability = C::ID.capability().as_str(),
                        effective_priority = connector.priority_for(C::ID),
                        error = ?error,
                        "connector command failed; trying next connector"
                    );
                    last_connector = Some(index);
                    last_error = Some(error);
                }
            }
        }

        ChainAttempt::Exhausted {
            last_connector,
            last_error,
        }
    }
}

struct RpcHealthCommand;

impl SourceCommand for RpcHealthCommand {
    type Output = ();

    const ID: CommandId = CommandId::RpcHealth;

    fn execute<'a>(
        &'a self,
        connector: &'a Connector,
    ) -> impl Future<Output = Result<Self::Output>> + Send + 'a {
        async move { connector.rpc()?.ensure_healthy().await }
    }
}

struct WebsocketHealthCommand;

impl SourceCommand for WebsocketHealthCommand {
    type Output = ();

    const ID: CommandId = CommandId::WsHealth;

    fn execute<'a>(
        &'a self,
        connector: &'a Connector,
    ) -> impl Future<Output = Result<Self::Output>> + Send + 'a {
        async move { connector.websocket()?.probe().await }
    }
}

#[cfg(test)]
mod tests {
    use super::{Connector, SourceChain, SourceCommand};
    use crate::{
        config::{Config, ConnectorConfig, ConnectorKind},
        provider::{Capability, CapabilityPriorityOverrides, CommandId, CommandPriorityOverrides},
    };
    use anyhow::{anyhow, bail, Result};
    use std::{
        future::Future,
        sync::{Arc, Mutex},
    };

    struct RecordingCommand {
        attempts: Arc<Mutex<Vec<String>>>,
        failures: Vec<&'static str>,
    }

    impl SourceCommand for RecordingCommand {
        type Output = String;

        const ID: CommandId = CommandId::CurrentLeaderSchedule;

        fn execute<'a>(
            &'a self,
            connector: &'a Connector,
        ) -> impl Future<Output = Result<Self::Output>> + Send + 'a {
            async move {
                let name = connector.name.to_string();
                self.attempts
                    .lock()
                    .map_err(|_| anyhow!("attempt recorder lock poisoned"))?
                    .push(name.clone());
                if self.failures.contains(&name.as_str()) {
                    bail!("connector {name} failed");
                }
                Ok(name)
            }
        }
    }

    #[tokio::test]
    async fn command_priority_then_capability_priority_then_base_priority() -> Result<()> {
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let chain = source_chain(vec![
            ConnectorConfig {
                name: "command".to_string(),
                priority: 20,
                capabilities: CapabilityPriorityOverrides::from([(Capability::LeaderSchedule, 40)]),
                commands: CommandPriorityOverrides::from([(CommandId::CurrentLeaderSchedule, 100)]),
                ..rpc_connector()
            },
            ConnectorConfig {
                name: "capability".to_string(),
                priority: 60,
                capabilities: CapabilityPriorityOverrides::from([(Capability::LeaderSchedule, 80)]),
                ..rpc_connector()
            },
            ConnectorConfig {
                name: "base".to_string(),
                priority: 70,
                ..rpc_connector()
            },
        ])?;
        let command = RecordingCommand {
            attempts: Arc::clone(&attempts),
            failures: vec!["command", "capability"],
        };

        let resolved = chain.required(&command).await?;

        assert_eq!(resolved.value, "base");
        assert_eq!(
            *attempts
                .lock()
                .map_err(|_| anyhow!("attempt recorder lock poisoned"))?,
            vec!["command", "capability", "base"]
        );
        Ok(())
    }

    #[tokio::test]
    async fn declaration_order_breaks_equal_priority_ties() -> Result<()> {
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let chain = source_chain(vec![
            ConnectorConfig {
                name: "first".to_string(),
                priority: 10,
                ..rpc_connector()
            },
            ConnectorConfig {
                name: "second".to_string(),
                priority: 10,
                ..rpc_connector()
            },
        ])?;
        let command = RecordingCommand {
            attempts: Arc::clone(&attempts),
            failures: vec!["first"],
        };

        let resolved = chain.required(&command).await?;

        assert_eq!(resolved.value, "second");
        assert_eq!(
            *attempts
                .lock()
                .map_err(|_| anyhow!("attempt recorder lock poisoned"))?,
            vec!["first", "second"]
        );
        Ok(())
    }

    #[tokio::test]
    async fn unsupported_connectors_are_not_attempted() -> Result<()> {
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let chain = SourceChain::from_config(&Config {
            validator_websocket_enabled: true,
            connectors: vec![
                ConnectorConfig {
                    name: "websocket-only".to_string(),
                    kind: ConnectorKind::HeliusEnhancedWss,
                    priority: 100,
                    ws_url: Some("wss://ws.example".to_string()),
                    ..ConnectorConfig::default()
                },
                ConnectorConfig {
                    name: "rpc".to_string(),
                    priority: 0,
                    ..rpc_connector()
                },
            ],
            ..Config::default()
        })?;
        let command = RecordingCommand {
            attempts: Arc::clone(&attempts),
            failures: vec![],
        };

        let resolved = chain.required(&command).await?;

        assert_eq!(resolved.value, "rpc");
        assert_eq!(
            *attempts
                .lock()
                .map_err(|_| anyhow!("attempt recorder lock poisoned"))?,
            vec!["rpc"]
        );
        Ok(())
    }

    #[tokio::test]
    async fn required_and_optional_commands_exhaust_the_same_chain() -> Result<()> {
        let chain = source_chain(vec![
            ConnectorConfig {
                name: "first".to_string(),
                priority: 20,
                ..rpc_connector()
            },
            ConnectorConfig {
                name: "last".to_string(),
                priority: 10,
                ..rpc_connector()
            },
        ])?;
        let command = RecordingCommand {
            attempts: Arc::new(Mutex::new(Vec::new())),
            failures: vec!["first", "last"],
        };

        assert!(chain.optional(&command).await.is_none());
        let error = match chain.required(&command).await {
            Ok(_) => bail!("required command unexpectedly succeeded"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("last connector was last"));
        Ok(())
    }

    fn source_chain(connectors: Vec<ConnectorConfig>) -> Result<SourceChain> {
        SourceChain::from_config(&Config {
            connectors,
            ..Config::default()
        })
    }

    fn rpc_connector() -> ConnectorConfig {
        ConnectorConfig {
            kind: ConnectorKind::StandardValidatorRpc,
            rpc_url: Some("http://127.0.0.1:8899".to_string()),
            ..ConnectorConfig::default()
        }
    }
}
