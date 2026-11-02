use crate::mode::{CommandId, ModeId};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
};

pub const ECONOMICS_TARGET_LABEL: &str = "focus:economics";

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum ConnectorKind {
    #[serde(rename = "standard-validator")]
    StandardValidator,
    #[serde(rename = "standard-validator+rpc")]
    #[default]
    StandardValidatorRpc,
    #[serde(rename = "standard-validator+full-rpc")]
    StandardValidatorFullRpc,
    #[serde(rename = "helius+rpc")]
    HeliusRpc,
    #[serde(rename = "helius+enhanced-rpc")]
    HeliusEnhancedRpc,
    #[serde(rename = "helius+ws")]
    HeliusWs,
    #[serde(rename = "helius+enhanced-wss")]
    HeliusEnhancedWss,
}

impl ConnectorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StandardValidator => "standard-validator",
            Self::StandardValidatorRpc => "standard-validator+rpc",
            Self::StandardValidatorFullRpc => "validator+full-rpc",
            Self::HeliusRpc => "helius+rpc",
            Self::HeliusEnhancedRpc => "helius+enhanced-rpc",
            Self::HeliusWs => "helius+ws",
            Self::HeliusEnhancedWss => "helius+enhanced-wss",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct HealthCommands {
    pub rpc_health: Vec<String>,
    pub ws_health: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConsensusCommands {
    pub epoch_info: Vec<String>,
    pub vote_accounts: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LeadershipCommands {
    pub current_block_production: Vec<String>,
    pub current_leader_schedule: Vec<String>,
    pub next_leader_schedule: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct EconomicsCommands {
    pub inflation_rewards: Vec<String>,
    pub reward_epoch_block_production: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct HealthModeConfig {
    pub commands: HealthCommands,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConsensusModeConfig {
    pub commands: ConsensusCommands,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LeadershipModeConfig {
    pub commands: LeadershipCommands,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct EconomicsModeConfig {
    pub enabled: bool,
    pub validator: Option<WatchedValidatorConfig>,
    pub commands: EconomicsCommands,
}

impl EconomicsModeConfig {
    pub const fn as_str(&self) -> &'static str {
        if self.enabled {
            "single-validator"
        } else {
            "disabled"
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModesConfig {
    pub health: HealthModeConfig,
    pub consensus: ConsensusModeConfig,
    pub leadership: LeadershipModeConfig,
    pub economics: EconomicsModeConfig,
}

impl ModesConfig {
    pub fn route(&self, command: CommandId) -> &[String] {
        match command {
            CommandId::RpcHealth => &self.health.commands.rpc_health,
            CommandId::WsHealth => &self.health.commands.ws_health,
            CommandId::EpochInfo => &self.consensus.commands.epoch_info,
            CommandId::VoteAccounts => &self.consensus.commands.vote_accounts,
            CommandId::CurrentBlockProduction => &self.leadership.commands.current_block_production,
            CommandId::CurrentLeaderSchedule => &self.leadership.commands.current_leader_schedule,
            CommandId::NextLeaderSchedule => &self.leadership.commands.next_leader_schedule,
            CommandId::InflationRewards => &self.economics.commands.inflation_rewards,
            CommandId::RewardEpochBlockProduction => {
                &self.economics.commands.reward_epoch_block_production
            }
        }
    }

    pub const fn enabled(&self, mode: ModeId) -> bool {
        match mode {
            ModeId::Health | ModeId::Consensus | ModeId::Leadership => true,
            ModeId::Economics => self.economics.enabled,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WatchedValidatorConfig {
    pub identity: Option<String>,
    pub vote_account: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConnectorConfig {
    pub name: String,
    pub kind: ConnectorKind,
    pub rpc_url: Option<String>,
    pub rpc_timeout_sec: u64,
    pub ws_url: Option<String>,
    pub ws_timeout_sec: u64,
    pub commitment: String,
}

impl Default for ConnectorConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: ConnectorKind::default(),
            rpc_url: None,
            rpc_timeout_sec: default_rpc_timeout_sec(),
            ws_url: None,
            ws_timeout_sec: default_ws_timeout_sec(),
            commitment: default_commitment(),
        }
    }
}

impl ConnectorConfig {
    pub fn has_rpc(&self) -> bool {
        self.rpc_url.is_some()
    }

    pub fn has_ws(&self) -> bool {
        self.ws_url.is_some()
    }

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
        self.rpc_url = self
            .rpc_url
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
        if self.rpc_url.is_none() && self.ws_url.is_none() {
            bail!("connectors[{index}] must define rpc_url and/or ws_url");
        }
        if self.rpc_timeout_sec == 0 {
            bail!("connectors[{index}].rpc_timeout_sec must be greater than 0");
        }
        if self.ws_timeout_sec == 0 {
            bail!("connectors[{index}].ws_timeout_sec must be greater than 0");
        }

        self.commitment_config()?;
        if let Some(ws_url) = self.ws_url.as_deref() {
            validate_ws_url(ws_url)?;
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub network: String,
    pub connectors: Vec<ConnectorConfig>,
    pub modes: ModesConfig,
    pub metrics_port: u16,
    pub poll_interval_sec: u64,
    pub delinquent_slot_distance: Option<u64>,
    pub liveness_file: Option<PathBuf>,
    pub watched_validators: Vec<WatchedValidatorConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            network: default_network(),
            connectors: vec![],
            modes: ModesConfig::default(),
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

fn default_ws_timeout_sec() -> u64 {
    10
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
                CommandId::WsHealth => false,
                CommandId::InflationRewards | CommandId::RewardEpochBlockProduction => {
                    self.modes.economics.enabled
                }
                _ => true,
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
                let transport_available =
                    matches!(command, CommandId::WsHealth) || connector.has_rpc();
                if !transport_available {
                    bail!(
                        "{path} references connector {:?}, which cannot run command {}",
                        connector.name,
                        command.as_str()
                    );
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

fn validate_ws_url(url: &str) -> Result<()> {
    if url.starts_with("ws://") || url.starts_with("wss://") {
        return Ok(());
    }

    bail!("ws_url must start with ws:// or wss://");
}

#[cfg(test)]
mod tests {
    use super::{
        expand_env_vars_with, Config, ConnectorConfig, ConnectorKind, ConsensusCommands,
        ConsensusModeConfig, EconomicsCommands, EconomicsModeConfig, HealthCommands,
        HealthModeConfig, LeadershipCommands, LeadershipModeConfig, ModesConfig,
        WatchedValidatorConfig, ECONOMICS_TARGET_LABEL,
    };
    use anyhow::{Context, Result};
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn loads_toml_and_preserves_explicit_command_route_order() -> Result<()> {
        let test_dir = unique_test_dir("ordered-connectors");
        fs::create_dir_all(&test_dir)
            .with_context(|| format!("failed to create {}", test_dir.display()))?;
        let config_path = test_dir.join("watcher.toml");
        fs::write(
            &config_path,
            r#"
network = "testnet"

[[connectors]]
name = "local-validator"
kind = "standard-validator"
rpc_url = "http://127.0.0.1:8899"

[[connectors]]
name = "helius-rpc"
kind = "helius+enhanced-rpc"
rpc_url = "https://rpc.helius.example"

[[connectors]]
name = "helius-ws"
kind = "helius+enhanced-wss"
ws_url = "wss://ws.helius.example"

[[watched_validators]]
identity = "11111111111111111111111111111111"
labels = ["vc:validator-1", "region:sbg"]

[modes.health.commands]
rpc_health = ["local-validator", "helius-rpc"]
ws_health = ["helius-ws"]

[modes.consensus.commands]
epoch_info = ["local-validator", "helius-rpc"]
vote_accounts = ["local-validator", "helius-rpc"]

[modes.leadership.commands]
current_block_production = ["helius-rpc", "local-validator"]
current_leader_schedule = ["local-validator", "helius-rpc"]
next_leader_schedule = ["local-validator", "helius-rpc"]
"#,
        )
        .with_context(|| format!("failed to write {}", config_path.display()))?;

        let config = Config::load(&config_path)?;

        assert_eq!(config.network, "testnet");
        assert_eq!(config.connectors.len(), 3);
        assert_eq!(config.connectors[0].name, "local-validator");
        assert_eq!(config.connectors[0].kind, ConnectorKind::StandardValidator);
        assert_eq!(config.connectors[1].name, "helius-rpc");
        assert_eq!(config.connectors[1].kind, ConnectorKind::HeliusEnhancedRpc);
        assert_eq!(config.connectors[2].name, "helius-ws");
        assert_eq!(config.connectors[2].kind, ConnectorKind::HeliusEnhancedWss);
        assert_eq!(config.watched_validators.len(), 1);
        assert_eq!(
            config.modes.leadership.commands.current_block_production,
            ["helius-rpc", "local-validator"]
        );
        assert_eq!(config.modes.health.commands.ws_health, ["helius-ws"]);

        fs::remove_dir_all(&test_dir)
            .with_context(|| format!("failed to remove directory {}", test_dir.display()))?;
        Ok(())
    }

    #[test]
    fn loads_every_committed_profile() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let cases = [
            (
                "config/local/validator.toml",
                "mainnet-beta",
                "local-validator",
                ConnectorKind::StandardValidatorRpc,
                false,
            ),
            (
                "config/compose/validator.toml",
                "external-validator",
                "external-validator",
                ConnectorKind::StandardValidatorRpc,
                false,
            ),
            (
                "config/compose/surfpool.toml",
                "surfpool-localnet",
                "surfpool",
                ConnectorKind::StandardValidatorRpc,
                true,
            ),
            (
                "config/remote/devnet.toml",
                "devnet",
                "devnet-rpc",
                ConnectorKind::StandardValidatorRpc,
                true,
            ),
            (
                "config/remote/mainnet.toml",
                "mainnet-beta",
                "mainnet-rpc",
                ConnectorKind::HeliusRpc,
                true,
            ),
        ];

        for (relative_path, network, connector_name, connector_kind, has_ws) in cases {
            let path = root.join(relative_path);
            let config = Config::load_with(&path, |raw| {
                expand_env_vars_with(raw, |key| {
                    let value = match key {
                        "SOL_DEVNET_RPC_URL" | "SOL_MAINNET_RPC_URL" => {
                            Some("http://127.0.0.1:8899".to_string())
                        }
                        "SOL_DEVNET_WS_URL" | "SOL_MAINNET_WS_URL" => {
                            Some("ws://127.0.0.1:8900".to_string())
                        }
                        _ => None,
                    };
                    Ok(value)
                })
            })
            .with_context(|| format!("failed to load committed profile {}", path.display()))?;

            assert_eq!(config.network, network, "profile {relative_path}");
            assert_eq!(config.connectors.len(), 1, "profile {relative_path}");
            assert_eq!(
                config.connectors[0].name, connector_name,
                "profile {relative_path}"
            );
            assert_eq!(
                config.connectors[0].kind, connector_kind,
                "profile {relative_path}"
            );
            assert!(config.connectors[0].has_rpc(), "profile {relative_path}");
            assert_eq!(
                config.connectors[0].has_ws(),
                has_ws,
                "profile {relative_path}"
            );
        }

        Ok(())
    }

    #[test]
    fn rejects_non_toml_config() -> Result<()> {
        let error = match Config::load(Path::new("config.yaml")) {
            Ok(_) => anyhow::bail!("non-TOML config should be rejected"),
            Err(error) => error,
        };

        assert!(error
            .to_string()
            .contains("config file config.yaml must use the .toml extension"));
        Ok(())
    }

    #[test]
    fn rejects_noncanonical_connector_kind_aliases() -> Result<()> {
        let test_dir = unique_test_dir("connector-kind-alias");
        fs::create_dir_all(&test_dir)
            .with_context(|| format!("failed to create {}", test_dir.display()))?;
        let config_path = test_dir.join("watcher.toml");
        fs::write(
            &config_path,
            r#"
network = "testnet"

[[connectors]]
name = "helius"
kind = "helius-rpc"
rpc_url = "https://rpc.helius.example"
"#,
        )
        .with_context(|| format!("failed to write {}", config_path.display()))?;

        let result = Config::load(&config_path);
        fs::remove_dir_all(&test_dir)
            .with_context(|| format!("failed to remove directory {}", test_dir.display()))?;

        let error = match result {
            Ok(_) => anyhow::bail!("noncanonical connector kind should be rejected"),
            Err(error) => error,
        };
        assert!(format!("{error:#}").contains("unknown variant"));
        Ok(())
    }

    #[test]
    fn canonical_connector_kinds_deserialize_and_export_stable_labels() -> Result<()> {
        let cases = [
            (
                "standard-validator",
                ConnectorKind::StandardValidator,
                "standard-validator",
            ),
            (
                "standard-validator+rpc",
                ConnectorKind::StandardValidatorRpc,
                "standard-validator+rpc",
            ),
            (
                "standard-validator+full-rpc",
                ConnectorKind::StandardValidatorFullRpc,
                "validator+full-rpc",
            ),
            ("helius+rpc", ConnectorKind::HeliusRpc, "helius+rpc"),
            (
                "helius+enhanced-rpc",
                ConnectorKind::HeliusEnhancedRpc,
                "helius+enhanced-rpc",
            ),
            ("helius+ws", ConnectorKind::HeliusWs, "helius+ws"),
            (
                "helius+enhanced-wss",
                ConnectorKind::HeliusEnhancedWss,
                "helius+enhanced-wss",
            ),
        ];

        for (toml_kind, expected_kind, exported_label) in cases {
            let connector: ConnectorConfig = toml::from_str(&format!(
                r#"
name = "test"
kind = "{toml_kind}"
rpc_url = "https://rpc.example"
"#
            ))?;

            assert_eq!(connector.kind, expected_kind);
            assert_eq!(connector.kind.as_str(), exported_label);
        }

        Ok(())
    }

    #[test]
    fn rejects_removed_root_and_unknown_connector_fields() -> Result<()> {
        for (field_name, setting) in [
            ("validator_mode", "validator_mode = \"rpc\""),
            (
                "validator_websocket_enabled",
                "validator_websocket_enabled = true",
            ),
        ] {
            let error = config_load_error(
                field_name,
                &format!(
                    r#"
network = "testnet"
{setting}

[[connectors]]
name = "primary"
kind = "standard-validator+rpc"
rpc_url = "https://rpc.example"
"#
                ),
            )?;
            let message = format!("{error:#}");
            assert!(
                message.contains(&format!("unknown field `{field_name}`")),
                "{message}"
            );
        }

        let connector_error = config_load_error(
            "unknown-connector-field",
            r#"
network = "testnet"

[[connectors]]
name = "primary"
kind = "standard-validator+rpc"
rpc_url = "https://rpc.example"
transport_mode = "poll-http"
"#,
        )?;
        let connector_message = format!("{connector_error:#}");
        assert!(
            connector_message.contains("unknown field `transport_mode`"),
            "{connector_message}"
        );

        Ok(())
    }

    #[test]
    fn rejects_missing_connector_list() -> Result<()> {
        let mut config = Config::default();

        match config.validate() {
            Ok(()) => anyhow::bail!("missing connectors should be rejected"),
            Err(error) => assert!(error
                .to_string()
                .contains("at least one connector is required")),
        }
        Ok(())
    }

    #[test]
    fn rejects_ws_only_connector_list() -> Result<()> {
        let mut config = Config {
            connectors: vec![ConnectorConfig {
                name: "ws-only".to_string(),
                kind: ConnectorKind::HeliusEnhancedWss,
                ws_url: Some("wss://ws.helius.example".to_string()),
                ..ConnectorConfig::default()
            }],
            ..Config::default()
        };

        match config.validate() {
            Ok(()) => anyhow::bail!("a websocket-only list should be rejected"),
            Err(error) => assert!(error
                .to_string()
                .contains("at least one connector with rpc_url is required")),
        }
        Ok(())
    }

    #[test]
    fn rejects_duplicate_connector_names() -> Result<()> {
        let mut config = Config {
            connectors: vec![rpc_connector("primary"), rpc_connector("primary")],
            ..Config::default()
        };

        match config.validate() {
            Ok(()) => anyhow::bail!("duplicate connector names should be rejected"),
            Err(error) => assert!(error
                .to_string()
                .contains("duplicate connector name \"primary\"")),
        }
        Ok(())
    }

    #[test]
    fn rejects_invalid_ws_scheme() -> Result<()> {
        let mut connector = rpc_connector("primary");
        connector.ws_url = Some("https://api.mainnet-beta.solana.com".to_string());
        let mut config = Config {
            connectors: vec![connector],
            ..Config::default()
        };

        match config.validate() {
            Ok(()) => anyhow::bail!("invalid websocket URL should be rejected"),
            Err(error) => assert!(error
                .to_string()
                .contains("ws_url must start with ws:// or wss://")),
        }
        Ok(())
    }

    #[test]
    fn trims_blank_ws_url() -> Result<()> {
        let mut connector = rpc_connector("primary");
        connector.ws_url = Some("   ".to_string());
        let mut config = Config {
            connectors: vec![connector],
            modes: modes_for("primary", None, false),
            ..Config::default()
        };

        config.validate()?;

        assert!(config.connectors[0].ws_url.is_none());
        Ok(())
    }

    #[test]
    fn rejects_unknown_and_duplicate_command_route_connectors() -> Result<()> {
        let mut unknown = Config {
            connectors: vec![rpc_connector("primary")],
            modes: modes_for("primary", None, false),
            ..Config::default()
        };
        unknown.modes.consensus.commands.epoch_info = vec!["missing".to_string()];
        let Err(error) = unknown.validate() else {
            anyhow::bail!("unknown route connector should be rejected");
        };
        assert!(error
            .to_string()
            .contains("references unknown connector \"missing\""));

        let mut duplicate = Config {
            connectors: vec![rpc_connector("primary")],
            modes: modes_for("primary", None, false),
            ..Config::default()
        };
        duplicate.modes.consensus.commands.epoch_info =
            vec!["primary".to_string(), "primary".to_string()];
        let Err(error) = duplicate.validate() else {
            anyhow::bail!("duplicate route connector should be rejected");
        };
        assert!(error
            .to_string()
            .contains("contains duplicate connector name \"primary\""));
        Ok(())
    }

    #[test]
    fn rejects_rpc_command_routed_to_websocket_only_connector() -> Result<()> {
        let mut config = Config {
            connectors: vec![
                rpc_connector("primary"),
                ConnectorConfig {
                    name: "ws-only".to_string(),
                    kind: ConnectorKind::HeliusEnhancedWss,
                    ws_url: Some("wss://ws.example".to_string()),
                    ..ConnectorConfig::default()
                },
            ],
            modes: modes_for("primary", Some("ws-only"), false),
            ..Config::default()
        };
        config.modes.leadership.commands.current_block_production = vec!["ws-only".to_string()];

        let Err(error) = config.validate() else {
            anyhow::bail!("RPC command without RPC transport should be rejected");
        };
        assert!(error
            .to_string()
            .contains("cannot run command current-block-production"));
        Ok(())
    }

    #[test]
    fn rejects_economics_routes_while_mode_is_disabled() -> Result<()> {
        let mut config = Config {
            connectors: vec![rpc_connector("primary")],
            modes: modes_for("primary", None, false),
            ..Config::default()
        };
        config.modes.economics.commands.inflation_rewards = vec!["primary".to_string()];

        let Err(error) = config.validate() else {
            anyhow::bail!("disabled economics routes should be rejected");
        };
        assert!(error
            .to_string()
            .contains("requires modes.economics.enabled = true"));
        Ok(())
    }

    #[test]
    fn economics_single_validator_is_merged_into_watched_validators() -> Result<()> {
        let mut config = Config {
            connectors: vec![rpc_connector("primary")],
            modes: modes_for("primary", None, true),
            watched_validators: vec![WatchedValidatorConfig {
                identity: Some("11111111111111111111111111111111".to_string()),
                vote_account: None,
                labels: vec!["region:sbg".to_string()],
            }],
            ..Config::default()
        };
        config.modes.economics.validator = Some(WatchedValidatorConfig {
            identity: Some("11111111111111111111111111111111".to_string()),
            vote_account: None,
            labels: vec!["team:ops".to_string()],
        });

        config.validate()?;

        assert_eq!(config.watched_validators.len(), 1);
        assert!(config.watched_validators[0]
            .labels
            .contains(&"region:sbg".to_string()));
        assert!(config.watched_validators[0]
            .labels
            .contains(&"team:ops".to_string()));
        assert!(config.watched_validators[0]
            .labels
            .contains(&ECONOMICS_TARGET_LABEL.to_string()));
        Ok(())
    }

    #[test]
    fn rejects_economics_validator_without_single_validator_mode() -> Result<()> {
        let mut config = Config {
            connectors: vec![rpc_connector("primary")],
            modes: ModesConfig {
                economics: EconomicsModeConfig {
                    enabled: false,
                    validator: Some(WatchedValidatorConfig {
                        identity: None,
                        vote_account: Some("vote-1".to_string()),
                        labels: vec![],
                    }),
                    ..EconomicsModeConfig::default()
                },
                ..modes_for("primary", None, false)
            },
            ..Config::default()
        };

        match config.validate() {
            Ok(()) => anyhow::bail!("economics validator should be rejected"),
            Err(error) => assert!(error
                .to_string()
                .contains("modes.economics.validator requires modes.economics.enabled = true")),
        }
        Ok(())
    }

    #[test]
    fn rejects_missing_economics_validator_in_single_validator_mode() -> Result<()> {
        let mut config = Config {
            connectors: vec![rpc_connector("primary")],
            modes: ModesConfig {
                economics: EconomicsModeConfig {
                    enabled: true,
                    validator: None,
                    commands: EconomicsCommands {
                        inflation_rewards: vec!["primary".to_string()],
                        reward_epoch_block_production: vec!["primary".to_string()],
                    },
                },
                ..modes_for("primary", None, false)
            },
            ..Config::default()
        };

        match config.validate() {
            Ok(()) => anyhow::bail!("missing economics validator should be rejected"),
            Err(error) => assert!(error
                .to_string()
                .contains("modes.economics.enabled requires modes.economics.validator")),
        }
        Ok(())
    }

    #[test]
    fn expands_env_variables_with_defaults() -> Result<()> {
        let expanded = expand_env_vars_with(
            r#"
network = "${WATCHER_NETWORK:-localnet}"
poll_interval_sec = ${WATCHER_POLL_INTERVAL_SEC:-15}
rpc_url = "${WATCHER_RPC_URL}"
"#,
            |key| {
                let value = match key {
                    "WATCHER_RPC_URL" => Some("http://validator:8899".to_string()),
                    _ => None,
                };
                Ok(value)
            },
        )?;

        assert!(expanded.contains("network = \"localnet\""));
        assert!(expanded.contains("poll_interval_sec = 15"));
        assert!(expanded.contains("rpc_url = \"http://validator:8899\""));
        Ok(())
    }

    #[test]
    fn rejects_missing_env_variables_without_defaults() -> Result<()> {
        let error = match expand_env_vars_with("rpc_url = \"${WATCHER_RPC_URL}\"", |_| Ok(None)) {
            Ok(_) => anyhow::bail!("expansion should fail"),
            Err(error) => error,
        };

        assert!(error
            .to_string()
            .contains("environment variable WATCHER_RPC_URL is required but not set"));
        Ok(())
    }

    #[test]
    fn uses_default_for_empty_env_values() -> Result<()> {
        let expanded = expand_env_vars_with(
            "poll_interval_sec = ${WATCHER_POLL_INTERVAL_SEC:-15}",
            |_| Ok(Some(String::new())),
        )?;

        assert_eq!(expanded, "poll_interval_sec = 15");
        Ok(())
    }

    #[test]
    fn rejects_unterminated_env_expressions() -> Result<()> {
        let error = match expand_env_vars_with("rpc_url = \"${WATCHER_RPC_URL\"", |_| Ok(None)) {
            Ok(_) => anyhow::bail!("expansion should fail"),
            Err(error) => error,
        };

        assert!(error
            .to_string()
            .contains("unterminated environment variable expression"));
        Ok(())
    }

    fn config_load_error(prefix: &str, raw: &str) -> Result<anyhow::Error> {
        let test_dir = unique_test_dir(prefix);
        fs::create_dir_all(&test_dir)
            .with_context(|| format!("failed to create {}", test_dir.display()))?;
        let config_path = test_dir.join("watcher.toml");
        fs::write(&config_path, raw)
            .with_context(|| format!("failed to write {}", config_path.display()))?;

        let result = Config::load(&config_path);
        fs::remove_dir_all(&test_dir)
            .with_context(|| format!("failed to remove directory {}", test_dir.display()))?;

        match result {
            Ok(_) => anyhow::bail!("configuration should be rejected"),
            Err(error) => Ok(error),
        }
    }

    fn rpc_connector(name: &str) -> ConnectorConfig {
        ConnectorConfig {
            name: name.to_string(),
            kind: ConnectorKind::StandardValidatorRpc,
            rpc_url: Some("https://api.mainnet-beta.solana.com".to_string()),
            ..ConnectorConfig::default()
        }
    }

    fn modes_for(rpc: &str, ws: Option<&str>, economics_enabled: bool) -> ModesConfig {
        let rpc_route = || vec![rpc.to_string()];
        ModesConfig {
            health: HealthModeConfig {
                commands: HealthCommands {
                    rpc_health: rpc_route(),
                    ws_health: ws.into_iter().map(str::to_string).collect(),
                },
            },
            consensus: ConsensusModeConfig {
                commands: ConsensusCommands {
                    epoch_info: rpc_route(),
                    vote_accounts: rpc_route(),
                },
            },
            leadership: LeadershipModeConfig {
                commands: LeadershipCommands {
                    current_block_production: rpc_route(),
                    current_leader_schedule: rpc_route(),
                    next_leader_schedule: rpc_route(),
                },
            },
            economics: EconomicsModeConfig {
                enabled: economics_enabled,
                validator: None,
                commands: EconomicsCommands {
                    inflation_rewards: economics_enabled.then(rpc_route).unwrap_or_default(),
                    reward_epoch_block_production: economics_enabled
                        .then(rpc_route)
                        .unwrap_or_default(),
                },
            },
        }
    }

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        PathBuf::from("target").join(format!(
            "sol-validator-watcher-config-{}-{}-{}",
            prefix,
            std::process::id(),
            nonce
        ))
    }
}
