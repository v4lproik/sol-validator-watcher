use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum TransportMode {
    #[default]
    PollHttp,
    PollHttpAndWs,
}

impl TransportMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PollHttp => "poll-http",
            Self::PollHttpAndWs => "poll-http-and-ws",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
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
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Consensus => "consensus",
            Self::Rpc => "rpc",
            Self::ConsensusAndRpc => "consensus-and-rpc",
            Self::NonVoting => "non-voting",
            Self::Observer => "observer",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct WatchedValidatorConfig {
    pub identity: Option<String>,
    pub vote_account: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_network")]
    pub network: String,
    #[serde(default = "default_rpc_url")]
    pub rpc_url: String,
    #[serde(default = "default_rpc_timeout_sec")]
    pub rpc_timeout_sec: u64,
    #[serde(default)]
    pub transport_mode: TransportMode,
    pub ws_url: Option<String>,
    #[serde(default = "default_ws_timeout_sec")]
    pub ws_timeout_sec: u64,
    #[serde(default)]
    pub validator_mode: ValidatorMode,
    #[serde(default)]
    pub validator_websocket_enabled: bool,
    #[serde(default = "default_metrics_port")]
    pub metrics_port: u16,
    #[serde(default = "default_poll_interval_sec")]
    pub poll_interval_sec: u64,
    #[serde(default = "default_commitment")]
    pub commitment: String,
    pub delinquent_slot_distance: Option<u64>,
    pub liveness_file: Option<PathBuf>,
    #[serde(default)]
    pub watched_validators: Vec<WatchedValidatorConfig>,
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
            _ => serde_yaml::from_str::<Self>(&expanded)
                .with_context(|| format!("failed to parse YAML config {}", path.display()))?,
        };

        config.validate()?;
        Ok(config)
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
        self.network = self.network.trim().to_string();
        self.rpc_url = self.rpc_url.trim().to_string();
        self.commitment = self.commitment.trim().to_lowercase();
        self.ws_url = self
            .ws_url
            .as_ref()
            .map(|url| url.trim().to_string())
            .filter(|url| !url.is_empty());

        if self.network.is_empty() {
            bail!("network must not be empty");
        }
        if self.rpc_url.is_empty() {
            bail!("rpc_url must not be empty");
        }
        if self.rpc_timeout_sec == 0 {
            bail!("rpc_timeout_sec must be greater than 0");
        }
        if self.ws_timeout_sec == 0 {
            bail!("ws_timeout_sec must be greater than 0");
        }
        if self.metrics_port == 0 {
            bail!("metrics_port must be greater than 0");
        }
        if self.poll_interval_sec == 0 {
            bail!("poll_interval_sec must be greater than 0");
        }
        self.commitment_config()?;
        self.validate_transport()?;

        for watched in &mut self.watched_validators {
            if watched.identity.is_none() && watched.vote_account.is_none() {
                bail!("each watched validator entry must define identity and/or vote_account");
            }

            if let Some(identity) = watched.identity.as_ref() {
                watched.identity = Some(normalize_pubkey(identity, "identity")?);
            }
            if let Some(vote_account) = watched.vote_account.as_ref() {
                watched.vote_account = Some(normalize_pubkey(vote_account, "vote_account")?);
            }

            for label in &watched.labels {
                validate_label(label)?;
            }
            watched.labels.sort();
            watched.labels.dedup();
        }

        Ok(())
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
    use super::{expand_env_vars_with, Config, TransportMode, ValidatorMode};
    use anyhow::Result;

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
        cfg.validate()?;
        assert_eq!(cfg.network, "testnet");
        assert_eq!(cfg.watched_validators.len(), 1);
        assert_eq!(cfg.transport_mode, TransportMode::PollHttpAndWs);
        assert_eq!(cfg.validator_mode, ValidatorMode::ConsensusAndRpc);
        assert!(cfg.validator_websocket_enabled);

        Ok(())
    }

    #[test]
    fn rejects_ws_url_without_ws_transport_mode() -> Result<()> {
        let mut cfg = Config {
            network: "mainnet-beta".to_string(),
            rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
            rpc_timeout_sec: 30,
            transport_mode: TransportMode::PollHttp,
            ws_url: Some("wss://api.mainnet-beta.solana.com".to_string()),
            ws_timeout_sec: 10,
            validator_mode: ValidatorMode::Consensus,
            validator_websocket_enabled: false,
            metrics_port: 8000,
            poll_interval_sec: 15,
            commitment: "confirmed".to_string(),
            delinquent_slot_distance: None,
            liveness_file: None,
            watched_validators: vec![],
        };

        match cfg.validate() {
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
            network: "mainnet-beta".to_string(),
            rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
            rpc_timeout_sec: 30,
            transport_mode: TransportMode::PollHttpAndWs,
            ws_url: Some("https://api.mainnet-beta.solana.com".to_string()),
            ws_timeout_sec: 10,
            validator_mode: ValidatorMode::Consensus,
            validator_websocket_enabled: true,
            metrics_port: 8000,
            poll_interval_sec: 15,
            commitment: "confirmed".to_string(),
            delinquent_slot_distance: None,
            liveness_file: None,
            watched_validators: vec![],
        };

        match cfg.validate() {
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
            network: "mainnet-beta".to_string(),
            rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
            rpc_timeout_sec: 30,
            transport_mode: TransportMode::PollHttp,
            ws_url: Some("   ".to_string()),
            ws_timeout_sec: 10,
            validator_mode: ValidatorMode::Observer,
            validator_websocket_enabled: false,
            metrics_port: 8000,
            poll_interval_sec: 15,
            commitment: "confirmed".to_string(),
            delinquent_slot_distance: None,
            liveness_file: None,
            watched_validators: vec![],
        };

        cfg.validate()?;
        assert!(cfg.ws_url.is_none());
        assert_eq!(cfg.validator_mode.as_str(), "observer");
        assert_eq!(cfg.transport_mode.as_str(), "poll-http");

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
    }
}
