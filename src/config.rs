use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::{
    fs,
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

        let mut config = match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => serde_json::from_str::<Self>(&raw)
                .with_context(|| format!("failed to parse JSON config {}", path.display()))?,
            _ => serde_yaml::from_str::<Self>(&raw)
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
    use super::{Config, TransportMode, ValidatorMode};
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
}
