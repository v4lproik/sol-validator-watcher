//! Validated runtime configuration and transactional reload state.
//!
//! Reload candidates build and verify complete source chains before replacing the
//! last-known-good runtime.

use crate::{
    collector::ensure_economics_supported, config::Config, source::SourceChain,
    watcher_state::RpcTelemetry,
};
use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};
use tracing::{info, warn};

/// Last-known-good runtime configuration and its constructed source chain.
pub(super) struct RuntimeState {
    /// TOML profile watched for modification.
    pub(super) config_path: PathBuf,
    /// Active validated configuration.
    pub(super) config: Config,
    /// Active connector clients and compiled command routes.
    pub(super) sources: SourceChain,
    /// RPC event handle shared with newly constructed clients.
    pub(super) rpc_telemetry: RpcTelemetry,
    /// Modification time of the active profile revision.
    pub(super) config_modified: SystemTime,
    /// Startup HTTP port; port changes require process restart.
    pub(super) metrics_port: u16,
    /// Whether standard connector capability probes succeeded.
    pub(super) connector_capabilities_verified: bool,
    /// Whether enabled economics routes passed live validation.
    pub(super) economics_capabilities_verified: bool,
}

impl RuntimeState {
    /// Loads the initial config and constructs its source chain without live probes.
    pub(super) fn load(config_path: PathBuf, rpc_telemetry: RpcTelemetry) -> Result<Self> {
        let config = Config::load(&config_path)?;
        let config_modified = config_mtime(&config_path)?;
        let metrics_port = config.metrics_port;
        let sources = SourceChain::from_config(&config, rpc_telemetry.clone())?;

        Ok(Self {
            config_path,
            config,
            sources,
            rpc_telemetry,
            config_modified,
            metrics_port,
            connector_capabilities_verified: false,
            economics_capabilities_verified: false,
        })
    }

    /// Validates and atomically installs a changed profile.
    ///
    /// Connector and economics probes complete before active state is replaced.
    /// An invalid candidate leaves every active field unchanged.
    pub(super) async fn reload_if_needed(&mut self) -> Result<()> {
        let modified = config_mtime(&self.config_path)?;
        if modified == self.config_modified {
            return Ok(());
        }

        let next = Config::load(&self.config_path)?;
        if next.metrics_port != self.metrics_port {
            warn!(
                old_metrics_port = self.metrics_port,
                new_metrics_port = next.metrics_port,
                "metrics_port changed in config; restart the watcher to move the HTTP listener"
            );
        }

        let sources_changed =
            self.config.connectors != next.connectors || self.config.modes != next.modes;
        let next_sources = if sources_changed {
            let sources = SourceChain::from_config(&next, self.rpc_telemetry.clone())?;
            sources
                .verify_capabilities()
                .await
                .context("reload candidate connector capability verification failed")?;
            Some(sources)
        } else {
            None
        };
        let candidate_sources = next_sources.as_ref().unwrap_or(&self.sources);
        ensure_economics_supported(candidate_sources, &next)
            .await
            .context("reload candidate economics capability verification failed")?;
        let next_economics_capabilities_verified = next.modes.economics.enabled;

        info!(
            watched_validators = next.watched_validators.len(),
            connectors = next.connectors.len(),
            economics_mode = next.modes.economics.as_str(),
            "reloaded configuration"
        );

        if let Some(sources) = next_sources {
            self.sources = sources;
            self.connector_capabilities_verified = true;
        }
        self.config = next;
        self.config_modified = modified;
        self.economics_capabilities_verified = next_economics_capabilities_verified;
        Ok(())
    }
}
/// Returns the profile modification time with path-specific error context.
pub(super) fn config_mtime(path: &Path) -> Result<SystemTime> {
    fs::metadata(path)
        .with_context(|| format!("failed to stat config file {}", path.display()))?
        .modified()
        .with_context(|| format!("failed to read mtime for config file {}", path.display()))
}

#[cfg(test)]
mod tests;
