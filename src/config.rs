use crate::provider::{
    Capability, CapabilityPriorityOverrides, CommandPriorityOverrides, ProviderDriver,
};
use anyhow::{bail, Context, Result};
use figment::{
    providers::{Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransportMode {
    #[default]
    PollHttp,
    PollHttpAndWs,
}

impl TransportMode {
    #[cfg(test)]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PollHttp => "poll-http",
            Self::PollHttpAndWs => "poll-http-and-ws",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ValidatorMode {
    #[default]
    Consensus,
    Rpc,
    ConsensusAndRpc,
    NonVoting,
    Observer,
}

impl ValidatorMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Consensus => "consensus",
            Self::Rpc => "rpc",
            Self::ConsensusAndRpc => "consensus-and-rpc",
            Self::NonVoting => "non-voting",
            Self::Observer => "observer",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EconomicsMode {
    #[default]
    Disabled,
    SingleValidator,
}

impl EconomicsMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::SingleValidator => "single-validator",
        }
    }
}

pub const ECONOMICS_TARGET_LABEL: &str = "focus:economics";

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum ConnectorKind {
    #[serde(rename = "standard-validator")]
    StandardValidator,
    #[serde(
        rename = "standard-validator+rpc",
        alias = "validator+rpc",
        alias = "standard-validator-rpc"
    )]
    #[default]
    StandardValidatorRpc,
    #[serde(
        rename = "validator+full-rpc",
        alias = "validator+full_rpc",
        alias = "standard-validator+full-rpc",
        alias = "standard-validator+full_rpc",
        alias = "standard-validator-full-rpc"
    )]
    StandardValidatorFullRpc,
    #[serde(rename = "helius+rpc", alias = "helius-rpc")]
    HeliusRpc,
    #[serde(
        rename = "helius+enhanced-rpc",
        alias = "helius+enhanced_rpc",
        alias = "helius-enhanced-rpc"
    )]
    HeliusEnhancedRpc,
    #[serde(rename = "helius+ws", alias = "helius+wss", alias = "helius-ws")]
    HeliusWs,
    #[serde(
        rename = "helius+enhanced-wss",
        alias = "helius+enhanced_ws",
        alias = "helius+enhanced_wss",
        alias = "helius-enhanced-wss"
    )]
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

    pub const fn driver(self) -> ProviderDriver {
        match self {
            Self::StandardValidator
            | Self::StandardValidatorRpc
            | Self::StandardValidatorFullRpc
            | Self::HeliusRpc => ProviderDriver::StandardRpc,
            Self::HeliusEnhancedRpc => ProviderDriver::EnhancedRpc,
            Self::HeliusWs | Self::HeliusEnhancedWss => ProviderDriver::WebsocketProbe,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct EconomicsConfig {
    pub mode: EconomicsMode,
    pub validator: Option<WatchedValidatorConfig>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WatchedValidatorConfig {
    pub identity: Option<String>,
    pub vote_account: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct ValidatorConnectionConfig {
    pub rpc_url: String,
    pub rpc_timeout_sec: u64,
    pub transport_mode: TransportMode,
    pub ws_url: Option<String>,
    pub ws_timeout_sec: u64,
    pub commitment: String,
}

impl Default for ValidatorConnectionConfig {
    fn default() -> Self {
        Self {
            rpc_url: default_rpc_url(),
            rpc_timeout_sec: default_rpc_timeout_sec(),
            transport_mode: TransportMode::default(),
            ws_url: None,
            ws_timeout_sec: default_ws_timeout_sec(),
            commitment: default_commitment(),
        }
    }
}

impl ValidatorConnectionConfig {
    fn load_from_toml(path: &Path, defaults: Self) -> Result<Self> {
        let raw = fs::read_to_string(path).with_context(|| {
            format!(
                "failed to read validator connection TOML file {}",
                path.display()
            )
        })?;
        let expanded = expand_env_vars(&raw).with_context(|| {
            format!(
                "failed to expand environment variables in validator connection TOML {}",
                path.display()
            )
        })?;

        Figment::from(Serialized::defaults(defaults))
            .merge(Toml::string(&expanded))
            .extract::<Self>()
            .with_context(|| {
                format!(
                    "failed to parse validator connection TOML {}",
                    path.display()
                )
            })
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

    fn validate(&mut self) -> Result<()> {
        self.normalize();

        if self.rpc_url.is_empty() {
            bail!("rpc_url must not be empty");
        }
        if self.rpc_timeout_sec == 0 {
            bail!("rpc_timeout_sec must be greater than 0");
        }
        if self.ws_timeout_sec == 0 {
            bail!("ws_timeout_sec must be greater than 0");
        }

        self.commitment_config()?;
        self.validate_transport()?;
        Ok(())
    }

    fn normalize(&mut self) {
        self.rpc_url = self.rpc_url.trim().to_string();
        self.commitment = self.commitment.trim().to_lowercase();
        self.ws_url = self
            .ws_url
            .as_ref()
            .map(|url| url.trim().to_string())
            .filter(|url| !url.is_empty());
    }

    fn validate_transport(&self) -> Result<()> {
        match self.transport_mode {
            TransportMode::PollHttp => {
                if self.ws_url.is_some() {
                    bail!("ws_url requires transport_mode: poll-http-and-ws");
                }
            }
            TransportMode::PollHttpAndWs => {
                let ws_url = self.ws_url.as_ref().with_context(|| {
                    "transport_mode poll-http-and-ws requires a non-empty ws_url"
                })?;
                validate_ws_url(ws_url)?;
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct ConnectorConfig {
    pub name: String,
    pub kind: ConnectorKind,
    pub priority: i32,
    pub capabilities: CapabilityPriorityOverrides,
    pub commands: CommandPriorityOverrides,
    pub rpc_url: Option<String>,
    pub rpc_timeout_sec: u64,
    pub ws_url: Option<String>,
    pub ws_timeout_sec: u64,
    pub commitment: String,
    pub connection_file: Option<PathBuf>,
}

impl Default for ConnectorConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: ConnectorKind::default(),
            priority: 0,
            capabilities: CapabilityPriorityOverrides::default(),
            commands: CommandPriorityOverrides::default(),
            rpc_url: None,
            rpc_timeout_sec: default_rpc_timeout_sec(),
            ws_url: None,
            ws_timeout_sec: default_ws_timeout_sec(),
            commitment: default_commitment(),
            connection_file: None,
        }
    }
}

impl ConnectorConfig {
    pub(crate) fn from_validator_connection(
        connection: &ValidatorConnectionConfig,
        connection_file: Option<PathBuf>,
    ) -> Self {
        Self {
            name: "primary".to_string(),
            kind: ConnectorKind::StandardValidatorRpc,
            priority: 0,
            capabilities: CapabilityPriorityOverrides::default(),
            commands: CommandPriorityOverrides::default(),
            rpc_url: Some(connection.rpc_url.clone()),
            rpc_timeout_sec: connection.rpc_timeout_sec,
            ws_url: connection.ws_url.clone(),
            ws_timeout_sec: connection.ws_timeout_sec,
            commitment: connection.commitment.clone(),
            connection_file,
        }
    }

    fn load_from_toml(path: &Path, defaults: Self) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read connector TOML file {}", path.display()))?;
        let expanded = expand_env_vars(&raw).with_context(|| {
            format!(
                "failed to expand environment variables in connector TOML {}",
                path.display()
            )
        })?;

        Figment::from(Serialized::defaults(defaults))
            .merge(Toml::string(&expanded))
            .extract::<Self>()
            .with_context(|| format!("failed to parse connector TOML {}", path.display()))
    }

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

    pub fn to_validator_connection(&self) -> Option<ValidatorConnectionConfig> {
        self.rpc_url
            .as_ref()
            .map(|rpc_url| ValidatorConnectionConfig {
                rpc_url: rpc_url.clone(),
                rpc_timeout_sec: self.rpc_timeout_sec,
                transport_mode: if self.ws_url.is_some() {
                    TransportMode::PollHttpAndWs
                } else {
                    TransportMode::PollHttp
                },
                ws_url: self.ws_url.clone(),
                ws_timeout_sec: self.ws_timeout_sec,
                commitment: self.commitment.clone(),
            })
    }

    fn supports_capability(&self, capability: Capability) -> bool {
        match capability {
            Capability::HealthRpc => {
                self.rpc_url.is_some() && self.kind.driver().supports(capability)
            }
            Capability::HealthWs => self.ws_url.is_some(),
            _ => self.rpc_url.is_some() && self.kind.driver().supports(capability),
        }
    }

    fn validate_priority_overrides(&self, index: usize) -> Result<()> {
        for capability in self.capabilities.keys().copied() {
            if !self.supports_capability(capability) {
                bail!(
                    "connectors[{index}].capabilities configures unsupported capability {}",
                    capability.as_str()
                );
            }
        }

        for command in self.commands.keys().copied() {
            if !self.supports_capability(command.capability()) {
                bail!(
                    "connectors[{index}].commands configures unsupported command {}",
                    command.as_str()
                );
            }
        }

        Ok(())
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
        self.validate_priority_overrides(index)?;

        self.commitment_config()?;
        if let Some(ws_url) = self.ws_url.as_deref() {
            validate_ws_url(ws_url)?;
        }

        Ok(())
    }

    fn resolve_connection_file(&mut self, config_path: &Path) -> Result<()> {
        let Some(connection_path) = self.connection_file.as_ref() else {
            return Ok(());
        };

        let resolved_path = resolve_config_relative_path(config_path, connection_path);
        let mut defaults = self.clone();
        defaults.connection_file = None;
        defaults.normalize();
        *self = Self::load_from_toml(&resolved_path, defaults)?;
        self.connection_file = Some(resolved_path);
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    pub network: String,
    #[serde(flatten)]
    pub validator_connection: ValidatorConnectionConfig,
    pub validator_connection_file: Option<PathBuf>,
    pub connectors: Vec<ConnectorConfig>,
    pub validator_mode: ValidatorMode,
    pub economics: EconomicsConfig,
    pub validator_websocket_enabled: bool,
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
            validator_connection: ValidatorConnectionConfig::default(),
            validator_connection_file: None,
            connectors: vec![],
            validator_mode: ValidatorMode::default(),
            economics: EconomicsConfig::default(),
            validator_websocket_enabled: false,
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

fn default_rpc_url() -> String {
    "http://127.0.0.1:8899".to_string()
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
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read config file {}", path.display()))?;
        let expanded = expand_env_vars(&raw).with_context(|| {
            format!(
                "failed to expand environment variables in {}",
                path.display()
            )
        })?;

        let mut config = match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => serde_json::from_str::<Self>(&expanded)
                .with_context(|| format!("failed to parse JSON config {}", path.display()))?,
            Some("toml") => Figment::new()
                .merge(Toml::string(&expanded))
                .extract::<Self>()
                .with_context(|| format!("failed to parse TOML config {}", path.display()))?,
            _ => serde_yaml::from_str::<Self>(&expanded)
                .with_context(|| format!("failed to parse YAML config {}", path.display()))?,
        };

        config.validate(path)?;
        Ok(config)
    }

    fn validate(&mut self, config_path: &Path) -> Result<()> {
        self.network = self.network.trim().to_string();
        if self.network.is_empty() {
            bail!("network must not be empty");
        }

        self.resolve_validator_connection(config_path)?;
        self.validator_connection.validate()?;
        self.resolve_connectors(config_path)?;

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
        Ok(())
    }

    pub fn effective_connectors(&self) -> Vec<ConnectorConfig> {
        if self.connectors.is_empty() {
            vec![ConnectorConfig::from_validator_connection(
                &self.validator_connection,
                self.validator_connection_file.clone(),
            )]
        } else {
            self.connectors.clone()
        }
    }

    fn validate_economics(&mut self) -> Result<()> {
        match self.economics.mode {
            EconomicsMode::Disabled => {
                if self.economics.validator.is_some() {
                    bail!("economics.validator requires economics.mode: single-validator");
                }
            }
            EconomicsMode::SingleValidator => {
                let validator = self.economics.validator.as_mut().with_context(|| {
                    "economics.mode single-validator requires economics.validator"
                })?;
                normalize_watched_validator(validator, "economics.validator")?;
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
            }
        }

        Ok(())
    }

    fn resolve_validator_connection(&mut self, config_path: &Path) -> Result<()> {
        let Some(connection_path) = self.validator_connection_file.as_ref() else {
            return Ok(());
        };

        let resolved_path = resolve_config_relative_path(config_path, connection_path);
        let mut defaults = self.validator_connection.clone();
        defaults.normalize();
        self.validator_connection =
            ValidatorConnectionConfig::load_from_toml(&resolved_path, defaults)?;
        self.validator_connection_file = Some(resolved_path);
        Ok(())
    }

    fn resolve_connectors(&mut self, config_path: &Path) -> Result<()> {
        if self.connectors.is_empty() {
            self.connectors
                .push(ConnectorConfig::from_validator_connection(
                    &self.validator_connection,
                    self.validator_connection_file.clone(),
                ));
        }

        for (index, connector) in self.connectors.iter_mut().enumerate() {
            connector.resolve_connection_file(config_path)?;
            connector.validate(index)?;
        }

        let mut seen_names = std::collections::HashSet::new();
        for connector in &self.connectors {
            if !seen_names.insert(connector.name.clone()) {
                bail!("duplicate connector name {:?}", connector.name);
            }
        }

        let primary = self
            .connectors
            .iter()
            .find_map(ConnectorConfig::to_validator_connection)
            .with_context(|| "at least one connector with rpc_url is required")?;
        self.validator_connection = primary;
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

fn resolve_config_relative_path(config_path: &Path, relative_path: &Path) -> PathBuf {
    if relative_path.is_absolute() {
        return relative_path.to_path_buf();
    }

    config_path.parent().map_or_else(
        || relative_path.to_path_buf(),
        |parent| parent.join(relative_path),
    )
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
        expand_env_vars_with, Config, ConnectorKind, EconomicsConfig, EconomicsMode, TransportMode,
        ValidatorConnectionConfig, ValidatorMode, WatchedValidatorConfig, ECONOMICS_TARGET_LABEL,
    };
    use crate::provider::{
        Capability, CapabilityPriorityOverrides, CommandId, CommandPriorityOverrides,
    };
    use anyhow::{bail, Context, Result};
    use std::{
        env, fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn parses_and_validates_yaml() -> Result<()> {
        let raw = r#"
network: testnet
rpc_url: https://api.testnet.solana.com
transport_mode: poll-http-and-ws
ws_url: wss://api.testnet.solana.com
validator_mode: consensus-and-rpc
validator_websocket_enabled: true
watched_validators:
  - identity: 11111111111111111111111111111111
    labels:
      - vc:validator-1
      - region:sbg
"#;

        let mut cfg: Config = serde_yaml::from_str(raw)?;
        cfg.validate(Path::new("config.yaml"))?;
        assert_eq!(cfg.network, "testnet");
        assert_eq!(cfg.watched_validators.len(), 1);
        assert_eq!(
            cfg.validator_connection.transport_mode,
            TransportMode::PollHttpAndWs
        );
        assert_eq!(cfg.validator_mode, ValidatorMode::ConsensusAndRpc);
        assert!(cfg.validator_websocket_enabled);

        Ok(())
    }

    #[test]
    fn loads_validator_connection_from_relative_toml() -> Result<()> {
        let test_dir = unique_test_dir("validator-connection");
        fs::create_dir_all(&test_dir)
            .with_context(|| format!("failed to create {}", test_dir.display()))?;
        let config_path = test_dir.join("watcher.yaml");
        let connection_path = test_dir.join("validator.toml");

        fs::write(
            &config_path,
            r#"
network: devnet
validator_connection_file: validator.toml
rpc_timeout_sec: 45
validator_mode: rpc
"#,
        )
        .with_context(|| format!("failed to write {}", config_path.display()))?;
        fs::write(
            &connection_path,
            r#"
rpc_url = "https://api.devnet.solana.com"
transport_mode = "poll-http-and-ws"
ws_url = "wss://api.devnet.solana.com"
"#,
        )
        .with_context(|| format!("failed to write {}", connection_path.display()))?;

        let cfg = Config::load(&config_path)?;

        assert_eq!(cfg.network, "devnet");
        assert_eq!(
            cfg.validator_connection.rpc_url,
            "https://api.devnet.solana.com"
        );
        assert_eq!(cfg.validator_connection.rpc_timeout_sec, 45);
        assert_eq!(
            cfg.validator_connection.transport_mode,
            TransportMode::PollHttpAndWs
        );
        assert_eq!(
            cfg.validator_connection.ws_url.as_deref(),
            Some("wss://api.devnet.solana.com")
        );
        assert_eq!(
            cfg.validator_connection_file.as_deref(),
            Some(connection_path.as_path())
        );

        fs::remove_dir_all(&test_dir)
            .with_context(|| format!("failed to remove directory {}", test_dir.display()))?;
        Ok(())
    }

    #[test]
    fn rejects_ws_url_without_ws_transport_mode() -> Result<()> {
        let mut cfg = Config {
            validator_connection: ValidatorConnectionConfig {
                rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
                ws_url: Some("wss://api.mainnet-beta.solana.com".to_string()),
                ..ValidatorConnectionConfig::default()
            },
            ..Config::default()
        };

        match cfg.validate(Path::new("config.yaml")) {
            Ok(()) => anyhow::bail!("config should be rejected"),
            Err(err) => assert!(err
                .to_string()
                .contains("ws_url requires transport_mode: poll-http-and-ws")),
        }

        Ok(())
    }

    #[test]
    fn rejects_invalid_ws_scheme() -> Result<()> {
        let mut cfg = Config {
            validator_connection: ValidatorConnectionConfig {
                rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
                transport_mode: TransportMode::PollHttpAndWs,
                ws_url: Some("https://api.mainnet-beta.solana.com".to_string()),
                ..ValidatorConnectionConfig::default()
            },
            ..Config::default()
        };

        match cfg.validate(Path::new("config.yaml")) {
            Ok(()) => anyhow::bail!("config should be rejected"),
            Err(err) => assert!(err
                .to_string()
                .contains("ws_url must start with ws:// or wss://")),
        }

        Ok(())
    }

    #[test]
    fn trims_blank_ws_url_when_ws_transport_is_disabled() -> Result<()> {
        let mut cfg = Config {
            validator_connection: ValidatorConnectionConfig {
                rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
                ws_url: Some("   ".to_string()),
                ..ValidatorConnectionConfig::default()
            },
            validator_mode: ValidatorMode::Observer,
            ..Config::default()
        };

        cfg.validate(Path::new("config.yaml"))?;
        assert!(cfg.validator_connection.ws_url.is_none());
        assert_eq!(cfg.validator_mode.as_str(), "observer");
        assert_eq!(
            cfg.validator_connection.transport_mode.as_str(),
            "poll-http"
        );

        Ok(())
    }

    #[test]
    fn economics_single_validator_is_merged_into_watched_validators() -> Result<()> {
        let mut cfg = Config {
            validator_connection: ValidatorConnectionConfig {
                rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
                ..ValidatorConnectionConfig::default()
            },
            watched_validators: vec![WatchedValidatorConfig {
                identity: Some("11111111111111111111111111111111".to_string()),
                vote_account: None,
                labels: vec!["region:sbg".to_string()],
            }],
            economics: EconomicsConfig {
                mode: EconomicsMode::SingleValidator,
                validator: Some(WatchedValidatorConfig {
                    identity: Some("11111111111111111111111111111111".to_string()),
                    vote_account: None,
                    labels: vec!["team:ops".to_string()],
                }),
            },
            ..Config::default()
        };

        cfg.validate(Path::new("config.yaml"))?;

        assert_eq!(cfg.economics.mode, EconomicsMode::SingleValidator);
        assert_eq!(cfg.watched_validators.len(), 1);
        assert_eq!(
            cfg.watched_validators[0].identity.as_deref(),
            Some("11111111111111111111111111111111")
        );
        assert!(cfg.watched_validators[0]
            .labels
            .contains(&"region:sbg".to_string()));
        assert!(cfg.watched_validators[0]
            .labels
            .contains(&"team:ops".to_string()));
        assert!(cfg.watched_validators[0]
            .labels
            .contains(&ECONOMICS_TARGET_LABEL.to_string()));

        Ok(())
    }

    #[test]
    fn parses_connector_stack_and_preserves_precedence_order() -> Result<()> {
        let raw = r#"
network: mainnet-beta
validator_websocket_enabled: true
connectors:
  - name: local-validator
    kind: standard-validator
    priority: 100
    rpc_url: http://127.0.0.1:8899
  - name: helius-rpc
    kind: helius+enhanced_rpc
    priority: 50
    capabilities:
      leader_schedule: 90
    commands:
      next_leader_schedule: 100
    rpc_url: https://rpc.helius.example
  - name: helius-ws
    kind: helius+enhanced_wss
    ws_url: wss://ws.helius.example
"#;

        let mut cfg: Config = serde_yaml::from_str(raw)?;
        cfg.validate(Path::new("config.yaml"))?;

        assert_eq!(cfg.connectors.len(), 3);
        assert_eq!(cfg.connectors[0].name, "local-validator");
        assert_eq!(cfg.connectors[0].kind, ConnectorKind::StandardValidator);
        assert_eq!(cfg.connectors[0].priority, 100);
        assert_eq!(cfg.connectors[1].kind, ConnectorKind::HeliusEnhancedRpc);
        assert_eq!(
            cfg.connectors[1].capabilities,
            CapabilityPriorityOverrides::from([(Capability::LeaderSchedule, 90)])
        );
        assert_eq!(
            cfg.connectors[1].commands,
            CommandPriorityOverrides::from([(CommandId::NextLeaderSchedule, 100)])
        );
        assert_eq!(cfg.connectors[2].kind, ConnectorKind::HeliusEnhancedWss);
        assert_eq!(
            cfg.validator_connection.rpc_url,
            "http://127.0.0.1:8899".to_string()
        );

        Ok(())
    }

    #[test]
    fn rejects_command_priority_for_unsupported_connector() -> Result<()> {
        let raw = r#"
network: mainnet-beta
validator_websocket_enabled: true
connectors:
  - name: websocket-only
    kind: helius+enhanced_wss
    ws_url: wss://ws.helius.example
    commands:
      epoch_info: 100
  - name: rpc
    kind: standard-validator+rpc
    rpc_url: https://rpc.example
"#;
        let mut config: Config = serde_yaml::from_str(raw)?;

        let error = match config.validate(Path::new("config.yaml")) {
            Ok(()) => bail!("unsupported command priority unexpectedly validated"),
            Err(error) => error,
        };

        assert!(error
            .to_string()
            .contains("configures unsupported command epoch-info"));
        Ok(())
    }

    #[test]
    fn effective_connectors_falls_back_to_legacy_validator_connection() {
        let config = Config {
            validator_connection: ValidatorConnectionConfig {
                rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
                ws_url: Some("wss://api.mainnet-beta.solana.com".to_string()),
                transport_mode: TransportMode::PollHttpAndWs,
                ..ValidatorConnectionConfig::default()
            },
            ..Config::default()
        };

        let connectors = config.effective_connectors();

        assert_eq!(connectors.len(), 1);
        assert_eq!(connectors[0].name, "primary");
        assert_eq!(connectors[0].kind, ConnectorKind::StandardValidatorRpc);
        assert_eq!(
            connectors[0].rpc_url.as_deref(),
            Some("https://api.mainnet-beta.solana.com")
        );
        assert_eq!(
            connectors[0].ws_url.as_deref(),
            Some("wss://api.mainnet-beta.solana.com")
        );
    }

    #[test]
    fn rejects_economics_validator_without_single_validator_mode() -> Result<()> {
        let mut cfg = Config {
            validator_connection: ValidatorConnectionConfig {
                rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
                ..ValidatorConnectionConfig::default()
            },
            economics: EconomicsConfig {
                mode: EconomicsMode::Disabled,
                validator: Some(WatchedValidatorConfig {
                    identity: None,
                    vote_account: Some("vote-1".to_string()),
                    labels: vec![],
                }),
            },
            ..Config::default()
        };

        match cfg.validate(Path::new("config.yaml")) {
            Ok(()) => anyhow::bail!("config should be rejected"),
            Err(err) => assert!(err
                .to_string()
                .contains("economics.validator requires economics.mode: single-validator")),
        }

        Ok(())
    }

    #[test]
    fn rejects_missing_economics_validator_in_single_validator_mode() -> Result<()> {
        let mut cfg = Config {
            validator_connection: ValidatorConnectionConfig {
                rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
                ..ValidatorConnectionConfig::default()
            },
            economics: EconomicsConfig {
                mode: EconomicsMode::SingleValidator,
                validator: None,
            },
            ..Config::default()
        };

        match cfg.validate(Path::new("config.yaml")) {
            Ok(()) => anyhow::bail!("config should be rejected"),
            Err(err) => assert!(err
                .to_string()
                .contains("economics.mode single-validator requires economics.validator")),
        }

        Ok(())
    }

    #[test]
    fn expands_env_variables_with_defaults() -> Result<()> {
        let expanded = expand_env_vars_with(
            r#"
network: ${WATCHER_NETWORK:-localnet}
rpc_url: ${WATCHER_RPC_URL}
validator_websocket_enabled: ${WATCHER_VALIDATOR_WEBSOCKET_ENABLED:-true}
"#,
            |key| {
                let value = match key {
                    "WATCHER_RPC_URL" => Some("http://validator:8899".to_string()),
                    _ => None,
                };
                Ok(value)
            },
        )?;

        assert!(expanded.contains("network: localnet"));
        assert!(expanded.contains("rpc_url: http://validator:8899"));
        assert!(expanded.contains("validator_websocket_enabled: true"));
        Ok(())
    }

    #[test]
    fn rejects_missing_env_variables_without_defaults() -> Result<()> {
        let err = match expand_env_vars_with("rpc_url: ${WATCHER_RPC_URL}", |_| Ok(None)) {
            Ok(_) => anyhow::bail!("expansion should fail"),
            Err(err) => err,
        };

        assert!(err
            .to_string()
            .contains("environment variable WATCHER_RPC_URL is required but not set"));
        Ok(())
    }

    #[test]
    fn uses_default_for_empty_env_values() -> Result<()> {
        let expanded = expand_env_vars_with(
            "validator_websocket_enabled: ${WATCHER_VALIDATOR_WEBSOCKET_ENABLED:-false}",
            |_| Ok(Some(String::new())),
        )?;

        assert_eq!(expanded, "validator_websocket_enabled: false");
        Ok(())
    }

    #[test]
    fn rejects_unterminated_env_expressions() -> Result<()> {
        let err = match expand_env_vars_with("rpc_url: ${WATCHER_RPC_URL", |_| Ok(None)) {
            Ok(_) => anyhow::bail!("expansion should fail"),
            Err(err) => err,
        };

        assert!(err
            .to_string()
            .contains("unterminated environment variable expression"));
        Ok(())
    }

    #[test]
    fn enum_string_helpers_cover_supported_modes() {
        assert_eq!(TransportMode::PollHttp.as_str(), "poll-http");
        assert_eq!(TransportMode::PollHttpAndWs.as_str(), "poll-http-and-ws");
        assert_eq!(ValidatorMode::Consensus.as_str(), "consensus");
        assert_eq!(ValidatorMode::Rpc.as_str(), "rpc");
        assert_eq!(ValidatorMode::ConsensusAndRpc.as_str(), "consensus-and-rpc");
        assert_eq!(ValidatorMode::NonVoting.as_str(), "non-voting");
        assert_eq!(ValidatorMode::Observer.as_str(), "observer");
        assert_eq!(EconomicsMode::Disabled.as_str(), "disabled");
        assert_eq!(EconomicsMode::SingleValidator.as_str(), "single-validator");
    }

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        env::temp_dir().join(format!(
            "sol-validator-watcher-config-{}-{}-{}",
            prefix,
            std::process::id(),
            nonce
        ))
    }
}
