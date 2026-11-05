//! Self-contained TOML profile loading, normalization, and validation.
//!
//! Configuration is expanded and validated before runtime construction. Command
//! arrays are the complete ordered fallback policy for their commands.

use crate::mode::{CommandId, ModeId, ProviderFeature, SolanaRpcCompatibility, Transport};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    env, fs,
    net::{IpAddr, Ipv4Addr},
    path::{Path, PathBuf},
};

/// Reserved label that identifies the validator used for economics collection.
pub const ECONOMICS_TARGET_LABEL: &str = "focus:economics";

/// Ordered connector routes for health commands.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct HealthCommands {
    /// Connector names used for HTTP RPC health, in fallback order.
    pub rpc_health: Vec<String>,
    /// Connector names used for WebSocket health, in fallback order.
    pub ws_health: Vec<String>,
    /// Connector names used for node-local status, in fallback order.
    pub node_status: Vec<String>,
}

/// Ordered connector routes for consensus commands.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConsensusCommands {
    /// Connector names used for epoch information, in fallback order.
    pub epoch_info: Vec<String>,
    /// Connector names used for vote accounts, in fallback order.
    pub vote_accounts: Vec<String>,
    /// Connector names used for recent performance samples, in fallback order.
    pub recent_performance_samples: Vec<String>,
}

/// Ordered connector routes for leadership commands.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LeadershipCommands {
    /// Connector names used for current block production, in fallback order.
    pub current_block_production: Vec<String>,
    /// Connector names used for the current leader schedule, in fallback order.
    pub current_leader_schedule: Vec<String>,
    /// Connector names used for the next leader schedule, in fallback order.
    pub next_leader_schedule: Vec<String>,
}

/// Ordered connector routes for economics commands.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct EconomicsCommands {
    /// Connector names used for identity and vote-account balances.
    pub account_balances: Vec<String>,
    /// Connector names used for prior-epoch inflation rewards.
    pub inflation_rewards: Vec<String>,
    /// Connector names used for prior-epoch block production.
    pub reward_epoch_block_production: Vec<String>,
    /// Connector names used for settled Jito MEV rewards.
    pub jito_mev_rewards: Vec<String>,
}

/// Health-mode command routes.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct HealthModeConfig {
    /// Commands owned by health mode.
    pub commands: HealthCommands,
}

/// Consensus-mode command routes.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConsensusModeConfig {
    /// Commands owned by consensus mode.
    pub commands: ConsensusCommands,
}

/// Leadership-mode command routes.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LeadershipModeConfig {
    /// Commands owned by leadership mode.
    pub commands: LeadershipCommands,
}

/// Optional single-validator economics mode and its command routes.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct EconomicsModeConfig {
    /// Whether economics collection is active.
    pub enabled: bool,
    /// Explicit economics target, or `None` to resolve it from the reserved label.
    pub validator: Option<WatchedValidatorConfig>,
    /// Commands owned by economics mode.
    pub commands: EconomicsCommands,
}

impl EconomicsModeConfig {
    /// Returns the stable mode label used in configuration metadata.
    pub const fn as_str(&self) -> &'static str {
        if self.enabled {
            "single-validator"
        } else {
            "disabled"
        }
    }
}

/// Complete collection-mode configuration.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModesConfig {
    /// Health-mode routes.
    pub health: HealthModeConfig,
    /// Consensus-mode routes.
    pub consensus: ConsensusModeConfig,
    /// Leadership-mode routes.
    pub leadership: LeadershipModeConfig,
    /// Economics-mode enablement, target, and routes.
    pub economics: EconomicsModeConfig,
}

impl ModesConfig {
    /// Returns the ordered connector names configured for a command.
    pub fn route(&self, command: CommandId) -> &[String] {
        match command {
            CommandId::RpcHealth => &self.health.commands.rpc_health,
            CommandId::WsHealth => &self.health.commands.ws_health,
            CommandId::NodeStatus => &self.health.commands.node_status,
            CommandId::EpochInfo => &self.consensus.commands.epoch_info,
            CommandId::VoteAccounts => &self.consensus.commands.vote_accounts,
            CommandId::RecentPerformanceSamples => {
                &self.consensus.commands.recent_performance_samples
            }
            CommandId::CurrentBlockProduction => &self.leadership.commands.current_block_production,
            CommandId::CurrentLeaderSchedule => &self.leadership.commands.current_leader_schedule,
            CommandId::NextLeaderSchedule => &self.leadership.commands.next_leader_schedule,
            CommandId::AccountBalances => &self.economics.commands.account_balances,
            CommandId::InflationRewards => &self.economics.commands.inflation_rewards,
            CommandId::RewardEpochBlockProduction => {
                &self.economics.commands.reward_epoch_block_production
            }
            CommandId::JitoMevRewards => &self.economics.commands.jito_mev_rewards,
        }
    }

    /// Reports whether a mode participates in collection.
    pub const fn enabled(&self, mode: ModeId) -> bool {
        match mode {
            ModeId::Health | ModeId::Consensus | ModeId::Leadership => true,
            ModeId::Economics => self.economics.enabled,
        }
    }
}

/// Validator selector and metric labels from one `[[watched_validators]]` entry.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WatchedValidatorConfig {
    /// Optional identity account address.
    pub identity: Option<String>,
    /// Optional vote-account address.
    pub vote_account: Option<String>,
    #[serde(default)]
    /// Additional validated `category:value` labels.
    pub labels: Vec<String>,
}

/// One connector-local RPC, WebSocket, and/or provider-API declaration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorConfig {
    /// Stable name referenced by command routes.
    pub name: String,
    /// Operator-facing provider name used in metrics and diagnostics.
    pub provider: String,
    /// Explicit non-standard provider capabilities.
    pub features: Vec<ProviderFeature>,
    #[serde(default)]
    /// Standard Solana RPC method profile, required when `rpc_url` is present.
    pub solana_rpc_compatibility: Option<SolanaRpcCompatibility>,
    #[serde(default)]
    /// HTTP JSON-RPC endpoint.
    pub rpc_url: Option<String>,
    #[serde(default = "default_rpc_timeout_sec")]
    /// HTTP JSON-RPC request timeout in seconds.
    pub rpc_timeout_sec: u64,
    #[serde(default)]
    /// Base URL for provider-specific HTTP API commands.
    pub provider_api_url: Option<String>,
    #[serde(default = "default_provider_api_timeout_sec")]
    /// Provider-specific HTTP API request timeout in seconds.
    pub provider_api_timeout_sec: u64,
    #[serde(default)]
    /// WebSocket endpoint used only for connectivity probes.
    pub ws_url: Option<String>,
    #[serde(default = "default_ws_timeout_sec")]
    /// WebSocket connection timeout in seconds.
    pub ws_timeout_sec: u64,
    #[serde(default = "default_commitment")]
    /// Solana commitment level sent with compatible RPC requests.
    pub commitment: String,
}

impl Default for ConnectorConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            provider: String::new(),
            features: Vec::new(),
            solana_rpc_compatibility: None,
            rpc_url: None,
            rpc_timeout_sec: default_rpc_timeout_sec(),
            provider_api_url: None,
            provider_api_timeout_sec: default_provider_api_timeout_sec(),
            ws_url: None,
            ws_timeout_sec: default_ws_timeout_sec(),
            commitment: default_commitment(),
        }
    }
}

impl ConnectorConfig {
    /// Reports whether this connector has an HTTP RPC endpoint.
    pub fn has_rpc(&self) -> bool {
        self.rpc_url.is_some()
    }

    /// Reports whether this connector has a provider-specific HTTP API endpoint.
    pub fn has_provider_api(&self) -> bool {
        self.provider_api_url.is_some()
    }

    /// Reports whether this connector has a WebSocket endpoint.
    pub fn has_ws(&self) -> bool {
        self.ws_url.is_some()
    }

    /// Reports whether this connector declares a provider feature.
    pub fn supports(&self, feature: ProviderFeature) -> bool {
        self.features.contains(&feature)
    }

    /// Returns a validated Solana commitment string.
    pub fn commitment_config(&self) -> Result<String> {
        match self.commitment.as_str() {
            "processed" | "confirmed" | "finalized" => Ok(self.commitment.clone()),
            other => bail!(
                "unsupported commitment {:?}, expected one of processed|confirmed|finalized",
                other
            ),
        }
    }

    fn normalize(&mut self) {
        self.name = self.name.trim().to_string();
        self.provider = self.provider.trim().to_string();
        self.rpc_url = self
            .rpc_url
            .as_ref()
            .map(|url| url.trim().to_string())
            .filter(|url| !url.is_empty());
        self.provider_api_url = self
            .provider_api_url
            .as_ref()
            .map(|url| url.trim().to_string())
            .filter(|url| !url.is_empty());
        self.commitment = self.commitment.trim().to_lowercase();
        self.ws_url = self
            .ws_url
            .as_ref()
            .map(|url| url.trim().to_string())
            .filter(|url| !url.is_empty());
    }

    fn validate(&mut self, index: usize) -> Result<()> {
        self.normalize();

        if self.name.is_empty() {
            bail!("connectors[{index}].name must not be empty");
        }
        if self.provider.is_empty() {
            bail!("connectors[{index}].provider must not be empty");
        }
        let mut seen_features = HashSet::with_capacity(self.features.len());
        for feature in &self.features {
            if !seen_features.insert(*feature) {
                bail!(
                    "connectors[{index}].features contains duplicate provider feature {:?}",
                    feature.as_str()
                );
            }
        }
        match (self.rpc_url.is_some(), self.solana_rpc_compatibility) {
            (true, None) => bail!(
                "connectors[{index}].solana_rpc_compatibility is required when rpc_url is configured"
            ),
            (false, Some(_)) => bail!(
                "connectors[{index}].solana_rpc_compatibility requires a configured rpc_url"
            ),
            _ => {}
        }
        if self.supports(ProviderFeature::HistoricalData)
            && self.solana_rpc_compatibility != Some(SolanaRpcCompatibility::Extended)
        {
            bail!(
                "connectors[{index}].features entry {:?} requires solana_rpc_compatibility = \"extended\", got {:?}",
                ProviderFeature::HistoricalData.as_str(),
                self.solana_rpc_compatibility
                    .map(SolanaRpcCompatibility::as_str)
            );
        }
        if self.supports(ProviderFeature::JitoMevRewards) && !self.has_provider_api() {
            bail!(
                "connectors[{index}].features entry {:?} requires provider_api_url",
                ProviderFeature::JitoMevRewards.as_str()
            );
        }
        if self.rpc_url.is_none() && self.ws_url.is_none() && self.provider_api_url.is_none() {
            bail!("connectors[{index}] must define rpc_url, ws_url, and/or provider_api_url");
        }
        if self.rpc_timeout_sec == 0 {
            bail!("connectors[{index}].rpc_timeout_sec must be greater than 0");
        }
        if self.provider_api_timeout_sec == 0 {
            bail!("connectors[{index}].provider_api_timeout_sec must be greater than 0");
        }
        if self.ws_timeout_sec == 0 {
            bail!("connectors[{index}].ws_timeout_sec must be greater than 0");
        }

        self.commitment_config()?;
        if let Some(rpc_url) = self.rpc_url.as_deref() {
            validate_http_url("rpc_url", rpc_url)?;
        }
        if let Some(ws_url) = self.ws_url.as_deref() {
            validate_ws_url(ws_url)?;
        }
        if let Some(provider_api_url) = self.provider_api_url.as_deref() {
            validate_http_url("provider_api_url", provider_api_url)?;
        }

        Ok(())
    }
}

/// Fully expanded, normalized, and validated watcher configuration.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Stable network label.
    pub network: String,
    /// Connector declarations in deterministic configuration order.
    pub connectors: Vec<ConnectorConfig>,
    /// Mode enablement and per-command fallback routes.
    pub modes: ModesConfig,
    /// IP address on which the metrics HTTP server listens.
    pub metrics_host: IpAddr,
    /// Port on which the metrics HTTP server listens.
    pub metrics_port: u16,
    /// Delay between completed polling rounds, in seconds.
    pub poll_interval_sec: u64,
    /// Optional cluster-specific delinquent vote distance.
    pub delinquent_slot_distance: Option<u64>,
    /// Optional file whose timestamp is refreshed after successful snapshots.
    pub liveness_file: Option<PathBuf>,
    /// Validators selected for detailed metrics and labels.
    pub watched_validators: Vec<WatchedValidatorConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            network: default_network(),
            connectors: vec![],
            modes: ModesConfig::default(),
            metrics_host: default_metrics_host(),
            metrics_port: default_metrics_port(),
            poll_interval_sec: default_poll_interval_sec(),
            delinquent_slot_distance: None,
            liveness_file: None,
            watched_validators: vec![],
        }
    }
}

fn default_network() -> String {
    "mainnet-beta".to_string()
}

fn default_rpc_timeout_sec() -> u64 {
    30
}

fn default_provider_api_timeout_sec() -> u64 {
    30
}

fn default_ws_timeout_sec() -> u64 {
    10
}

fn default_metrics_host() -> IpAddr {
    IpAddr::V4(Ipv4Addr::LOCALHOST)
}

fn default_metrics_port() -> u16 {
    8000
}

fn default_poll_interval_sec() -> u64 {
    15
}

fn default_commitment() -> String {
    "confirmed".to_string()
}

impl Config {
    /// Loads one TOML profile after environment expansion, normalization, and validation.
    pub fn load(path: &Path) -> Result<Self> {
        Self::load_with(path, expand_env_vars)
    }

    fn load_with<F>(path: &Path, expand: F) -> Result<Self>
    where
        F: FnOnce(&str) -> Result<String>,
    {
        if path.extension().and_then(|extension| extension.to_str()) != Some("toml") {
            bail!(
                "config file {} must use the .toml extension",
                path.display()
            );
        }

        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read config file {}", path.display()))?;
        let expanded = expand(&raw).with_context(|| {
            format!(
                "failed to expand environment variables in {}",
                path.display()
            )
        })?;
        let mut config = toml::from_str::<Self>(&expanded)
            .with_context(|| format!("failed to parse TOML config {}", path.display()))?;

        config.validate()?;
        Ok(config)
    }

    fn validate(&mut self) -> Result<()> {
        self.network = self.network.trim().to_string();
        if self.network.is_empty() {
            bail!("network must not be empty");
        }

        self.validate_connectors()?;
        if self.metrics_port == 0 {
            bail!("metrics_port must be greater than 0");
        }
        if self.poll_interval_sec == 0 {
            bail!("poll_interval_sec must be greater than 0");
        }

        for watched in &mut self.watched_validators {
            normalize_watched_validator(watched, "watched_validators entry")?;
        }

        self.validate_economics()?;
        self.validate_routes()?;
        Ok(())
    }

    fn validate_economics(&mut self) -> Result<()> {
        let economics = &mut self.modes.economics;
        if !economics.enabled {
            if economics.validator.is_some() {
                bail!("modes.economics.validator requires modes.economics.enabled = true");
            }
            return Ok(());
        }

        let validator = economics
            .validator
            .as_mut()
            .context("modes.economics.enabled requires modes.economics.validator")?;
        normalize_watched_validator(validator, "modes.economics.validator")?;
        validator.labels.push(ECONOMICS_TARGET_LABEL.to_string());
        validator.labels.sort();
        validator.labels.dedup();

        if let Some(existing) = self
            .watched_validators
            .iter_mut()
            .find(|watched| watched_validators_match(watched, validator))
        {
            existing.labels.extend(validator.labels.iter().cloned());
            existing.labels.sort();
            existing.labels.dedup();
        } else {
            self.watched_validators.push(validator.clone());
        }

        Ok(())
    }

    fn validate_routes(&self) -> Result<()> {
        for command in CommandId::ALL {
            let route = self.modes.route(command);
            let path = format!(
                "modes.{}.commands.{}",
                command.mode().as_str(),
                command.as_str().replace('-', "_")
            );
            let required = match command {
                CommandId::RpcHealth | CommandId::EpochInfo | CommandId::VoteAccounts => true,
                CommandId::WsHealth
                | CommandId::NodeStatus
                | CommandId::RecentPerformanceSamples
                | CommandId::CurrentBlockProduction
                | CommandId::CurrentLeaderSchedule
                | CommandId::NextLeaderSchedule
                | CommandId::JitoMevRewards => false,
                CommandId::AccountBalances
                | CommandId::InflationRewards
                | CommandId::RewardEpochBlockProduction => self.modes.economics.enabled,
            };

            if route.is_empty() {
                if required {
                    bail!("{path} must contain at least one connector name");
                }
                continue;
            }
            if command.mode() == ModeId::Economics && !self.modes.economics.enabled {
                bail!("{path} requires modes.economics.enabled = true");
            }

            let requirement = command.source_requirement();
            let mut seen = HashSet::with_capacity(route.len());
            for connector_name in route {
                if !seen.insert(connector_name) {
                    bail!("{path} contains duplicate connector name {connector_name:?}");
                }
                let connector = self
                    .connectors
                    .iter()
                    .find(|candidate| candidate.name == *connector_name)
                    .with_context(|| {
                        format!("{path} references unknown connector {connector_name:?}")
                    })?;
                match requirement.transport {
                    Transport::Rpc if !connector.has_rpc() => bail!(
                        "{path} references connector {:?}, which has no configured RPC endpoint",
                        connector.name
                    ),
                    Transport::ProviderApi if !connector.has_provider_api() => bail!(
                        "{path} references connector {:?}, which has no configured provider API endpoint",
                        connector.name
                    ),
                    _ => {}
                }
                if let Some(required_compatibility) = requirement.solana_rpc_compatibility {
                    let declared_compatibility =
                        connector.solana_rpc_compatibility.with_context(|| {
                            format!(
                                "{path} references connector {:?}, whose RPC endpoint has no declared solana_rpc_compatibility",
                                connector.name
                            )
                        })?;
                    if !declared_compatibility.supports(required_compatibility) {
                        bail!(
                            "{path} references connector {:?}, but command {:?} requires solana_rpc_compatibility = {:?} and the connector RPC endpoint declares {:?}",
                            connector.name,
                            command.as_str(),
                            required_compatibility.as_str(),
                            declared_compatibility.as_str()
                        );
                    }
                }
                if let Some(feature) = requirement.feature {
                    if !connector.supports(feature) {
                        bail!(
                            "{path} references connector {:?}, but command {:?} requires provider feature {:?}",
                            connector.name,
                            command.as_str(),
                            feature.as_str()
                        );
                    }
                }
            }
        }

        Ok(())
    }

    fn validate_connectors(&mut self) -> Result<()> {
        if self.connectors.is_empty() {
            bail!("at least one connector is required");
        }

        for (index, connector) in self.connectors.iter_mut().enumerate() {
            connector.validate(index)?;
        }

        let mut seen_names = std::collections::HashSet::new();
        for connector in &self.connectors {
            if !seen_names.insert(connector.name.as_str()) {
                bail!("duplicate connector name {:?}", connector.name);
            }
        }

        if !self.connectors.iter().any(ConnectorConfig::has_rpc) {
            bail!("at least one connector with rpc_url is required");
        }

        Ok(())
    }
}

fn expand_env_vars(input: &str) -> Result<String> {
    expand_env_vars_with(input, |key| match env::var(key) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => {
            bail!("environment variable {} is not valid UTF-8", key)
        }
    })
}

fn expand_env_vars_with<F>(input: &str, mut resolve: F) -> Result<String>
where
    F: FnMut(&str) -> Result<Option<String>>,
{
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '$' || chars.peek() != Some(&'{') {
            output.push(ch);
            continue;
        }

        let _ = chars.next();
        let mut expression = String::new();
        let mut closed = false;

        for next in chars.by_ref() {
            if next == '}' {
                closed = true;
                break;
            }
            expression.push(next);
        }

        if !closed {
            bail!(
                "unterminated environment variable expression starting with ${{{}}}",
                expression
            );
        }

        let (key, default) = match expression.split_once(":-") {
            Some((key, default)) => (key.trim(), Some(default)),
            None => (expression.trim(), None),
        };

        if key.is_empty() {
            bail!("environment variable expression must include a variable name");
        }

        let value = match resolve(key)? {
            Some(value) if !value.is_empty() => value,
            Some(_) | None => match default {
                Some(default) => default.to_string(),
                None => bail!("environment variable {} is required but not set", key),
            },
        };

        output.push_str(&value);
    }

    Ok(output)
}

fn normalize_pubkey(value: &str, field_name: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("{} must not be empty", field_name);
    }
    Ok(trimmed.to_string())
}

fn normalize_watched_validator(
    watched: &mut WatchedValidatorConfig,
    field_name: &str,
) -> Result<()> {
    if watched.identity.is_none() && watched.vote_account.is_none() {
        bail!("{field_name} must define identity and/or vote_account");
    }

    if let Some(identity) = watched.identity.as_ref() {
        watched.identity = Some(normalize_pubkey(
            identity,
            &format!("{field_name}.identity"),
        )?);
    }
    if let Some(vote_account) = watched.vote_account.as_ref() {
        watched.vote_account = Some(normalize_pubkey(
            vote_account,
            &format!("{field_name}.vote_account"),
        )?);
    }

    for label in &watched.labels {
        validate_label(label)?;
    }
    watched.labels.sort();
    watched.labels.dedup();
    Ok(())
}

fn watched_validators_match(left: &WatchedValidatorConfig, right: &WatchedValidatorConfig) -> bool {
    left.identity.is_some() && left.identity == right.identity
        || left.vote_account.is_some() && left.vote_account == right.vote_account
}

fn validate_label(label: &str) -> Result<()> {
    let trimmed = label.trim();
    let Some((category, value)) = trimmed.split_once(':') else {
        bail!("label {:?} must follow the category:value format", trimmed);
    };

    if category.is_empty() || value.is_empty() {
        bail!("label {:?} must follow the category:value format", trimmed);
    }

    Ok(())
}

fn validate_http_url(field: &str, url: &str) -> Result<()> {
    let parsed =
        reqwest::Url::parse(url).with_context(|| format!("{field} must be a valid URL"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        bail!("{field} must use http:// or https:// with a host");
    }
    Ok(())
}

fn validate_ws_url(url: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(url).context("ws_url must be a valid URL")?;
    if !matches!(parsed.scheme(), "ws" | "wss") || parsed.host_str().is_none() {
        bail!("ws_url must use ws:// or wss:// with a host");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
