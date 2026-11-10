//! Top-level runtime wiring for supervised process tasks.
//!
//! This module constructs the shared state once, registers every long-running
//! task with the task manager, and delegates shutdown coordination to it.

use super::{
    http::{bind_http_listener, run_http_server, AppState},
    poller::run_watcher_loop,
    state::RuntimeState,
};
use crate::{
    metrics::Metrics,
    tasks::{Shutdown, TaskExit, TaskManager},
    watcher_state::WatcherStateMailbox,
};
use anyhow::{Context, Result};
use std::{path::PathBuf, sync::Arc};
use tokio::signal;
use tracing::info;

/// Builds and supervises the watcher actor, poller, HTTP server, and signal task.
pub(crate) async fn run(config_path: PathBuf) -> Result<()> {
    let watcher_state_mailbox = WatcherStateMailbox::new();
    let runtime = RuntimeState::load(config_path, watcher_state_mailbox.runtime_telemetry())?;
    let metrics = Arc::new(Metrics::new()?);
    metrics.set_watcher_up(&runtime.config.network, true);
    metrics.set_rpc_up(&runtime.config.network, false);
    let (watcher_state, watcher_state_actor) = watcher_state_mailbox.start(
        &runtime.config.network,
        runtime.sources.failover_inventory(),
        Arc::clone(&metrics),
    );
    let server_state = AppState::new(Arc::clone(&metrics), watcher_state.clone());
    let server_listener = bind_http_listener(runtime.metrics_address).await?;
    let mut tasks = TaskManager::new();

    tasks.spawn(
        "watcher-state-actor",
        watcher_state_actor.run(tasks.shutdown_handle()),
    );
    tasks.spawn(
        "metrics-http-server",
        run_http_server(server_state, server_listener, tasks.shutdown_handle()),
    );
    tasks.spawn(
        "watcher-loop",
        run_watcher_loop(runtime, metrics, watcher_state, tasks.shutdown_handle()),
    );
    tasks.spawn(
        "shutdown-signal",
        wait_for_shutdown_signal(tasks.shutdown_handle()),
    );

    tasks.run().await
}

async fn wait_for_shutdown_signal(mut shutdown: Shutdown) -> Result<TaskExit> {
    tokio::select! {
        result = signal::ctrl_c() => {
            result.context("failed to listen for shutdown signal")?;
            info!("received shutdown signal");
            Ok(TaskExit::ShutdownRequested)
        }
        _ = shutdown.cancelled() => Ok(TaskExit::Finished),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::test_support::unique_test_dir;
    use anyhow::Context;
    use std::fs;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn signal_task_finishes_after_cooperative_shutdown() -> Result<()> {
        let mut tasks = TaskManager::new();
        tasks.spawn(
            "shutdown-signal",
            wait_for_shutdown_signal(tasks.shutdown_handle()),
        );
        tasks.spawn("shutdown-request", async {
            Ok(TaskExit::ShutdownRequested)
        });

        tasks.run().await
    }

    #[tokio::test]
    async fn occupied_metrics_address_fails_before_tasks_start() -> Result<()> {
        let occupied_listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let occupied_address = occupied_listener.local_addr()?;
        let test_dir = unique_test_dir("occupied-metrics-address");
        fs::create_dir_all(&test_dir)
            .with_context(|| format!("failed to create {}", test_dir.display()))?;
        let config_path = test_dir.join("watcher.toml");
        fs::write(
            &config_path,
            format!(
                r#"
network = "testnet"
metrics_host = "127.0.0.1"
metrics_port = {}
poll_interval_sec = 15

[[connectors]]
name = "primary"
provider = "test"
features = []
solana_rpc_compatibility = "extended"
rpc = {{ url = "http://127.0.0.1:8899" }}

[modes.health.commands]
rpc_health = ["primary"]

[modes.consensus.commands]
epoch_info = ["primary"]
vote_accounts = ["primary"]
recent_performance_samples = ["primary"]

[modes.leadership.commands]
current_block_production = ["primary"]
current_leader_schedule = ["primary"]
next_leader_schedule = ["primary"]

[modes.economics]
enabled = false
"#,
                occupied_address.port()
            ),
        )
        .with_context(|| format!("failed to write {}", config_path.display()))?;

        let error = run(config_path)
            .await
            .err()
            .context("occupied metrics address must fail startup")?;
        assert!(
            format!("{error:#}").contains(&format!(
                "failed to bind metrics server on {occupied_address}"
            )),
            "{error:#}"
        );

        fs::remove_dir_all(&test_dir)
            .with_context(|| format!("failed to remove directory {}", test_dir.display()))?;
        Ok(())
    }
}
