use crate::{
    metrics::Metrics,
    tasks::{Shutdown, TaskExit},
    watcher_state::{WatcherState, ADMISSION_CLEAN_POLL_ROUNDS},
};
use anyhow::{Context, Result};
use axum::{extract::State, http::StatusCode, routing::get, Router};
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;
use tracing::{error, info};

#[derive(Clone)]
pub(super) struct AppState {
    metrics: Arc<Metrics>,
    watcher_state: WatcherState,
}

impl AppState {
    pub(super) fn new(metrics: Arc<Metrics>, watcher_state: WatcherState) -> Self {
        Self {
            metrics,
            watcher_state,
        }
    }
}
pub(super) async fn run_http_server(
    state: AppState,
    metrics_port: u16,
    mut shutdown: Shutdown,
) -> Result<TaskExit> {
    let address = SocketAddr::from(([0, 0, 0, 0], metrics_port));
    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/healthz", get(health_handler))
        .route("/readyz", get(readiness_handler))
        .with_state(state);

    let listener = TcpListener::bind(address)
        .await
        .with_context(|| format!("failed to bind metrics server on {}", address))?;

    info!(address = %address, "serving metrics");
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown.cancelled().await;
        })
        .await
        .context("HTTP server failed")?;

    Ok(TaskExit::Finished)
}
async fn metrics_handler(
    State(state): State<AppState>,
) -> Result<(StatusCode, String), StatusCode> {
    state
        .metrics
        .render()
        .map(|body| (StatusCode::OK, body))
        .map_err(|err| {
            error!(error = ?err, "failed to render metrics");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn health_handler() -> &'static str {
    "ok"
}

pub(super) async fn readiness_handler(State(state): State<AppState>) -> (StatusCode, String) {
    let (ready, clean_poll_rounds) = state.watcher_state.readiness();
    if ready {
        (
            StatusCode::OK,
            format!(
                "ready: {clean_poll_rounds}/{ADMISSION_CLEAN_POLL_ROUNDS} consecutive clean polling rounds"
            ),
        )
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "warming up: {clean_poll_rounds}/{ADMISSION_CLEAN_POLL_ROUNDS} consecutive clean polling rounds"
            ),
        )
    }
}

#[cfg(test)]
mod tests;
