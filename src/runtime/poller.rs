//! Polling lifecycle, dependency guards, liveness-file updates, and recovery.
//!
//! Polls reload configuration transactionally, validate startup capabilities,
//! and commit only complete snapshots. Post-startup collection failures reset
//! readiness without replacing the last snapshot, then retry after bounded
//! exponential backoff.

use super::state::RuntimeState;
use crate::{
    collector::{
        collect_snapshot, ensure_economics_supported, ModeStatus, Snapshot, SnapshotDataSource,
    },
    config::Config,
    metrics::Metrics,
    mode::{CommandId, ModeId},
    tasks::{Shutdown, TaskExit},
    watcher_state::{WatcherState, WatcherStateError, ADMISSION_SUCCESSFUL_POLL_ROUNDS},
};
use anyhow::{Context, Result};
use std::{
    fs,
    path::Path,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::time::sleep;
use tracing::{info, warn};

const POLL_RETRY_POLICY: PollRetryPolicy = PollRetryPolicy {
    initial_delay: Duration::from_secs(1),
    max_delay: Duration::from_secs(30),
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PollRetryPolicy {
    initial_delay: Duration,
    max_delay: Duration,
}

impl PollRetryPolicy {
    fn delay(self, consecutive_failed_rounds: u32) -> Duration {
        debug_assert!(consecutive_failed_rounds > 0);
        let exponent = consecutive_failed_rounds.saturating_sub(1).min(31);
        self.initial_delay
            .saturating_mul(2_u32.pow(exponent))
            .min(self.max_delay)
    }
}

/// Runs polling rounds until shutdown, retrying post-startup collection failures.
///
/// Startup and configuration contract failures remain fatal. A failed runtime
/// collection resets readiness, retains the last successful snapshot, and waits
/// with bounded exponential backoff before starting a new polling round.
pub(super) async fn run_watcher_loop(
    runtime: RuntimeState,
    metrics: Arc<Metrics>,
    watcher_state: WatcherState,
    shutdown: Shutdown,
) -> Result<TaskExit> {
    run_watcher_loop_with_retry_policy(runtime, metrics, watcher_state, shutdown, POLL_RETRY_POLICY)
        .await
}

async fn run_watcher_loop_with_retry_policy(
    mut runtime: RuntimeState,
    metrics: Arc<Metrics>,
    watcher_state: WatcherState,
    mut shutdown: Shutdown,
    retry_policy: PollRetryPolicy,
) -> Result<TaskExit> {
    info!(
        network = runtime.config.network,
        watched_validators = runtime.config.watched_validators.len(),
        connectors = runtime.config.connectors.len(),
        economics_mode = runtime.config.modes.economics.as_str(),
        admission_successful_poll_rounds = ADMISSION_SUCCESSFUL_POLL_ROUNDS,
        "starting Solana validator watcher"
    );
    let mut consecutive_failed_poll_rounds = 0_u32;

    loop {
        let previous_config_modified = runtime.config_modified;
        match runtime.reload_if_needed().await {
            Ok(sources_changed) if runtime.config_modified != previous_config_modified => {
                metrics.record_config_reload(&runtime.config.network, "accepted");
                let reset = if sources_changed {
                    watcher_state
                        .reset_runtime(
                            &runtime.config.network,
                            runtime.sources.failover_inventory(),
                        )
                        .await
                } else {
                    watcher_state.reset_admission(&runtime.config.network).await
                };
                let Some(()) = state_transition_or_shutdown(reset, &shutdown)
                    .context("failed to reset readiness after configuration reload")?
                else {
                    return Ok(finish_watcher_shutdown(
                        metrics.as_ref(),
                        &runtime.config.network,
                    ));
                };
            }
            Ok(_) => {}
            Err(err) => {
                metrics.record_config_reload(&runtime.config.network, "rejected");
                warn!(error = ?err, "failed to reload config, keeping previous configuration");
            }
        }
        ensure_connector_capabilities_ready(&mut runtime).await?;
        ensure_economics_capabilities_ready(&mut runtime).await?;
        metrics.set_watcher_up(&runtime.config.network, true);
        metrics.begin_poll(&runtime.inventory);
        let Some(()) = state_transition_or_shutdown(watcher_state.begin_poll().await, &shutdown)
            .context("failed to begin polling round")?
        else {
            return Ok(finish_watcher_shutdown(
                metrics.as_ref(),
                &runtime.config.network,
            ));
        };
        let snapshot_started_at = Instant::now();
        let poll_result = collect_poll_snapshot(&runtime, metrics.as_ref()).await;
        let snapshot = match poll_result {
            Ok(snapshot) => snapshot,
            Err(error) => {
                mark_collection_modes_down(&runtime.config, metrics.as_ref());
                metrics.record_snapshot_failure(
                    &runtime.config.network,
                    snapshot_started_at.elapsed(),
                );
                let Some(completion) =
                    state_transition_or_shutdown(watcher_state.fail_poll().await, &shutdown)
                        .context("failed to publish failed polling round")?
                else {
                    return Ok(finish_watcher_shutdown(
                        metrics.as_ref(),
                        &runtime.config.network,
                    ));
                };
                consecutive_failed_poll_rounds = consecutive_failed_poll_rounds.saturating_add(1);
                let retry_delay = retry_policy.delay(consecutive_failed_poll_rounds);
                warn!(
                    error = ?error,
                    consecutive_failed_poll_rounds,
                    retry_delay_seconds = retry_delay.as_secs_f64(),
                    ready = completion.state.ready,
                    "polling round failed; last successful snapshot remains unchanged"
                );
                if shutdown_requested_before(&mut shutdown, retry_delay).await {
                    return Ok(finish_watcher_shutdown(
                        metrics.as_ref(),
                        &runtime.config.network,
                    ));
                }
                continue;
            }
        };
        let completed_at_unix_seconds = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_secs_f64(),
            Err(error) => {
                metrics.record_snapshot_failure(
                    &runtime.config.network,
                    snapshot_started_at.elapsed(),
                );
                return Err(error).context("system clock is before UNIX_EPOCH");
            }
        };
        metrics.update_snapshot(&snapshot);
        let Some(completion) =
            state_transition_or_shutdown(watcher_state.complete_poll().await, &shutdown)
                .context("failed to publish completed polling round")?
        else {
            return Ok(finish_watcher_shutdown(
                metrics.as_ref(),
                &runtime.config.network,
            ));
        };
        metrics.record_snapshot_success(
            &runtime.config.network,
            snapshot_started_at.elapsed(),
            completed_at_unix_seconds,
        );
        update_liveness_file(runtime.config.liveness_file.as_deref())?;
        consecutive_failed_poll_rounds = 0;
        let state = completion.state;
        if completion.admitted {
            info!(
                completed_poll_rounds = state.completed_poll_rounds,
                successful_poll_rounds = state.successful_poll_rounds,
                degraded = state.degraded,
                "watcher admitted as ready after consecutive successful polling rounds"
            );
        }
        info!(
            epoch = snapshot.epoch,
            slot = snapshot.slot,
            scopes = snapshot.scopes.len(),
            ready = state.ready,
            degraded = state.degraded,
            successful_poll_rounds = state.successful_poll_rounds,
            "updated Prometheus metrics"
        );

        if shutdown_requested_before(
            &mut shutdown,
            Duration::from_secs(runtime.config.poll_interval_sec),
        )
        .await
        {
            return Ok(finish_watcher_shutdown(
                metrics.as_ref(),
                &runtime.config.network,
            ));
        }
    }
}

async fn collect_poll_snapshot(runtime: &RuntimeState, metrics: &Metrics) -> Result<Snapshot> {
    let (health_sources, health_status) = guard_dependencies_healthy(runtime, metrics).await?;
    let mut snapshot = collect_snapshot(&runtime.sources, &runtime.config)
        .await
        .context("failed to collect snapshot from configured command routes")?;
    let health_status = ModeStatus {
        up: health_status.up
            && (runtime.config.modes.route(CommandId::NodeStatus).is_empty()
                || snapshot
                    .node
                    .as_ref()
                    .is_some_and(|node| node.health.is_healthy())),
        ..health_status
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

    Ok(snapshot)
}

async fn shutdown_requested_before(shutdown: &mut Shutdown, delay: Duration) -> bool {
    tokio::select! {
        _ = shutdown.cancelled() => true,
        _ = sleep(delay) => false,
    }
}

/// Preserves state-transition failures unless cooperative shutdown already won.
///
/// The actor and poller are sibling supervised tasks, so the actor may receive
/// shutdown immediately before the poller publishes its final transition.
fn state_transition_or_shutdown<T>(
    transition: std::result::Result<T, WatcherStateError>,
    shutdown: &Shutdown,
) -> Result<Option<T>> {
    match transition {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.is_actor_stopped() && shutdown.is_cancelled() => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn finish_watcher_shutdown(metrics: &Metrics, network: &str) -> TaskExit {
    metrics.set_watcher_up(network, false);
    info!("watcher loop shutting down");
    TaskExit::Finished
}

async fn guard_dependencies_healthy(
    runtime: &RuntimeState,
    metrics: &Metrics,
) -> Result<(Vec<SnapshotDataSource>, ModeStatus)> {
    metrics.set_rpc_up(&runtime.config.network, false);
    metrics.set_ws_up(&runtime.config.network, false);

    let rpc_health = match runtime.sources.rpc_health().await {
        Ok(health) => health,
        Err(error) => {
            metrics.set_mode_up(&runtime.config.network, ModeId::Health, false);
            return Err(error).context("RPC health command failed for all configured connectors");
        }
    };
    let rpc_up = rpc_health.value.is_healthy();
    metrics.set_rpc_up(&runtime.config.network, rpc_up);
    let (websocket_source, websocket_up) = match runtime.sources.websocket_healthy().await {
        Ok(Some(source)) => {
            metrics.set_ws_up(&runtime.config.network, true);
            (Some(source.into()), true)
        }
        Ok(None) => (None, true),
        Err(err) => {
            warn!(
                error = ?err,
                "websocket health command failed for all configured connectors"
            );
            (None, false)
        }
    };
    let sources = [Some(rpc_health.source.into()), websocket_source]
        .into_iter()
        .flatten()
        .collect();
    let health_up = rpc_up && websocket_up;
    metrics.set_mode_up(&runtime.config.network, ModeId::Health, health_up);

    Ok((
        sources,
        ModeStatus {
            mode: ModeId::Health,
            up: health_up,
        },
    ))
}

/// Marks every enabled collection mode except health as unavailable.
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

/// Performs the one-time live economics route check when economics is enabled.
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

/// Reports whether the active runtime still requires economics route verification.
pub(super) fn economics_capability_check_required(runtime: &RuntimeState) -> bool {
    !runtime.economics_capabilities_verified && runtime.config.modes.economics.enabled
}
/// Writes the current Unix timestamp to the configured liveness file.
///
/// A missing path disables the file. Parent directories are created as needed.
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
