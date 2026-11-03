use super::state::RuntimeState;
use crate::{
    collector::{
        collect_snapshot, command::GetHealth, ensure_economics_supported, ModeStatus,
        SnapshotDataSource,
    },
    config::Config,
    metrics::Metrics,
    mode::ModeId,
    tasks::{Shutdown, TaskExit},
    watcher_state::{WatcherState, ADMISSION_CLEAN_POLL_ROUNDS},
};
use anyhow::{Context, Result};
use std::{
    fs,
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::time::sleep;
use tracing::{info, warn};

pub(super) async fn run_watcher_loop(
    mut runtime: RuntimeState,
    metrics: Arc<Metrics>,
    watcher_state: WatcherState,
    mut shutdown: Shutdown,
) -> Result<TaskExit> {
    info!(
        network = runtime.config.network,
        watched_validators = runtime.config.watched_validators.len(),
        connectors = runtime.config.connectors.len(),
        economics_mode = runtime.config.modes.economics.as_str(),
        admission_clean_poll_rounds = ADMISSION_CLEAN_POLL_ROUNDS,
        "starting Solana validator watcher"
    );

    loop {
        let previous_config_modified = runtime.config_modified;
        match runtime.reload_if_needed().await {
            Ok(()) if runtime.config_modified != previous_config_modified => {
                watcher_state
                    .reset_admission(&runtime.config.network)
                    .await
                    .context("failed to reset readiness after configuration reload")?;
            }
            Ok(()) => {}
            Err(err) => {
                warn!(error = ?err, "failed to reload config, keeping previous configuration");
            }
        }
        ensure_connector_capabilities_ready(&mut runtime).await?;

        metrics.set_watcher_up(&runtime.config.network, true);
        metrics.begin_poll(&runtime.config, &runtime.sources);
        watcher_state
            .begin_poll()
            .await
            .context("failed to begin polling round")?;
        let (health_sources, health_status) =
            guard_dependencies_healthy(&runtime, metrics.as_ref()).await?;
        if let Err(error) = ensure_economics_capabilities_ready(&mut runtime).await {
            metrics.set_mode_up(&runtime.config.network, ModeId::Economics, false);
            return Err(error);
        }
        let mut snapshot = match collect_snapshot(&runtime.sources, &runtime.config).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                mark_collection_modes_down(&runtime.config, metrics.as_ref());
                return Err(error)
                    .context("failed to collect snapshot from configured command routes");
            }
        };
        snapshot.data_sources.extend(health_sources);
        snapshot.mode_statuses.push(health_status);
        if !runtime.config.watched_validators.is_empty() && snapshot.watched_validators.is_empty() {
            warn!(
                configured_watched_validators = runtime.config.watched_validators.len(),
                economics_mode = runtime.config.modes.economics.as_str(),
                "configured watched validators did not match any vote accounts in the latest snapshot; watched-validator panels will be empty"
            );
        }
        metrics.update_snapshot(&snapshot);
        update_liveness_file(runtime.config.liveness_file.as_deref())?;
        let completion = watcher_state
            .complete_poll()
            .await
            .context("failed to publish completed polling round")?;
        let state = completion.state;
        if completion.admitted {
            info!(
                completed_poll_rounds = state.completed_poll_rounds,
                clean_poll_rounds = state.clean_poll_rounds,
                "watcher admitted as ready after consecutive clean polling rounds"
            );
        }
        info!(
            epoch = snapshot.epoch,
            slot = snapshot.slot,
            scopes = snapshot.scopes.len(),
            ready = state.ready,
            clean_poll_rounds = state.clean_poll_rounds,
            "updated Prometheus metrics"
        );

        tokio::select! {
            _ = shutdown.cancelled() => {
                metrics.set_watcher_up(&runtime.config.network, false);
                info!("watcher loop shutting down");
                return Ok(TaskExit::Finished);
            }
            _ = sleep(Duration::from_secs(runtime.config.poll_interval_sec)) => {}
        }
    }
}

async fn guard_dependencies_healthy(
    runtime: &RuntimeState,
    metrics: &Metrics,
) -> Result<(Vec<SnapshotDataSource>, ModeStatus)> {
    metrics.set_rpc_up(&runtime.config.network, false);
    metrics.set_ws_up(&runtime.config.network, false);

    let rpc_health = match runtime.sources.required(&GetHealth).await {
        Ok(health) => health,
        Err(error) => {
            metrics.set_mode_up(&runtime.config.network, ModeId::Health, false);
            return Err(error).context("RPC health command failed for all configured connectors");
        }
    };
    metrics.set_rpc_up(&runtime.config.network, true);
    let mut sources = vec![rpc_health.source.into()];

    let health_up = match runtime.sources.websocket_healthy().await {
        Ok(Some(source)) => {
            metrics.set_ws_up(&runtime.config.network, true);
            sources.push(source.into());
            true
        }
        Ok(None) => true,
        Err(err) => {
            warn!(
                error = ?err,
                "websocket health command failed for all configured connectors"
            );
            false
        }
    };
    metrics.set_mode_up(&runtime.config.network, ModeId::Health, health_up);

    Ok((
        sources,
        ModeStatus {
            mode: ModeId::Health,
            up: health_up,
        },
    ))
}

pub(super) fn mark_collection_modes_down(config: &Config, metrics: &Metrics) {
    for mode in [ModeId::Consensus, ModeId::Leadership, ModeId::Economics] {
        if config.modes.enabled(mode) {
            metrics.set_mode_up(&config.network, mode, false);
        }
    }
}

async fn ensure_connector_capabilities_ready(runtime: &mut RuntimeState) -> Result<()> {
    if runtime.connector_capabilities_verified {
        return Ok(());
    }

    info!("validating declared connector RPC capabilities");
    runtime
        .sources
        .verify_capabilities()
        .await
        .context("failed to validate declared connector RPC capabilities")?;
    runtime.connector_capabilities_verified = true;
    info!("validated declared connector RPC capabilities");
    Ok(())
}

pub(super) async fn ensure_economics_capabilities_ready(runtime: &mut RuntimeState) -> Result<()> {
    if !economics_capability_check_required(runtime) {
        return Ok(());
    }

    info!(
        economics_mode = runtime.config.modes.economics.as_str(),
        "validating economics RPC capabilities"
    );

    ensure_economics_supported(&runtime.sources, &runtime.config)
        .await
        .context("failed to validate economics command routes")?;

    runtime.economics_capabilities_verified = true;
    info!("validated economics RPC capabilities");
    Ok(())
}

pub(super) fn economics_capability_check_required(runtime: &RuntimeState) -> bool {
    !runtime.economics_capabilities_verified && runtime.config.modes.economics.enabled
}
pub(super) fn update_liveness_file(path: Option<&Path>) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };

    let parent = path
        .parent()
        .with_context(|| format!("invalid liveness path {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "failed to create liveness parent directory {}",
            parent.display()
        )
    })?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before UNIX_EPOCH")?
        .as_secs()
        .to_string();
    fs::write(path, now)
        .with_context(|| format!("failed to update liveness file {}", path.display()))
}

#[cfg(test)]
mod tests;
