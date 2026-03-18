use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

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

        if self.network.is_empty() {
            bail!("network must not be empty");
        }
        if self.rpc_url.is_empty() {
            bail!("rpc_url must not be empty");
        }
        if self.rpc_timeout_sec == 0 {
            bail!("rpc_timeout_sec must be greater than 0");
        }
        if self.metrics_port == 0 {
            bail!("metrics_port must be greater than 0");
        }
        if self.poll_interval_sec == 0 {
            bail!("poll_interval_sec must be greater than 0");
        }
        self.commitment_config()?;

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

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn parses_and_validates_yaml() {
        let raw = r#"
network: testnet
rpc_url: https://api.testnet.solana.com
watched_validators:
  - identity: 11111111111111111111111111111111
    labels:
      - vc:validator-1
      - region:sbg
"#;

        let mut cfg: Config = serde_yaml::from_str(raw).expect("yaml should parse");
        cfg.validate().expect("config should validate");
        assert_eq!(cfg.network, "testnet");
        assert_eq!(cfg.watched_validators.len(), 1);
    }
}
