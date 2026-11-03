//! Top-level runtime wiring for supervised process tasks.
//!
//! This module constructs the shared state once, registers every long-running
//! task with the task manager, and delegates shutdown coordination to it.

use super::{
    http::{run_http_server, AppState},
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
    let runtime = RuntimeState::load(config_path, watcher_state_mailbox.rpc_telemetry())?;
    let metrics = Arc::new(Metrics::new()?);
    metrics.set_watcher_up(&runtime.config.network, true);
    metrics.set_rpc_up(&runtime.config.network, false);
    let (watcher_state, watcher_state_actor) =
        watcher_state_mailbox.start(&runtime.config.network, Arc::clone(&metrics));
    let server_state = AppState::new(Arc::clone(&metrics), watcher_state.clone());
    let mut tasks = TaskManager::new();

    tasks.spawn(
        "watcher-state-actor",
        watcher_state_actor.run(tasks.shutdown_handle()),
    );
    tasks.spawn(
        "metrics-http-server",
        run_http_server(server_state, runtime.metrics_port, tasks.shutdown_handle()),
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
