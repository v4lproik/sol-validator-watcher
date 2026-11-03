//! HTTP surfaces for Prometheus metrics, process health, and traffic readiness.
//!
//! `/healthz` reports process liveness, while `/readyz` reflects the watcher-state
//! actor's clean-poll admission contract.

use crate::{
    metrics::Metrics,
    tasks::{Shutdown, TaskExit},
    watcher_state::{WatcherState, ADMISSION_CLEAN_POLL_ROUNDS},
};
use anyhow::{bail, Context, Result};
use axum::{extract::State, http::StatusCode, routing::get, Router};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};
use tracing::{error, info};

const HEALTHCHECK_TIMEOUT: Duration = Duration::from_secs(2);
const HEALTHCHECK_REQUEST: &[u8] =
    b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
const HEALTHY_STATUS_PREFIX: &[u8; 12] = b"HTTP/1.1 200";

/// Shared HTTP handler state.
#[derive(Clone)]
pub(super) struct AppState {
    metrics: Arc<Metrics>,
    watcher_state: WatcherState,
}

impl AppState {
    /// Creates handler state from the metric registry and watcher-state handle.
    pub(super) fn new(metrics: Arc<Metrics>, watcher_state: WatcherState) -> Self {
        Self {
            metrics,
            watcher_state,
        }
    }
}

/// Serves metrics, liveness, and readiness until cooperative shutdown.
pub(super) async fn run_http_server(
    state: AppState,
    address: SocketAddr,
    mut shutdown: Shutdown,
) -> Result<TaskExit> {
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

/// Probes the process-liveness endpoint used by the container health check.
pub(crate) async fn check_health(address: SocketAddr) -> Result<()> {
    timeout(HEALTHCHECK_TIMEOUT, async {
        let mut stream = TcpStream::connect(address)
            .await
            .with_context(|| format!("failed to connect to health endpoint on {address}"))?;
        stream
            .write_all(HEALTHCHECK_REQUEST)
            .await
            .context("failed to send health request")?;
        let mut status_prefix = [0_u8; HEALTHY_STATUS_PREFIX.len()];
        stream
            .read_exact(&mut status_prefix)
            .await
            .context("failed to read health response status")?;
        if status_prefix != *HEALTHY_STATUS_PREFIX {
            bail!("health endpoint returned unexpected status prefix {status_prefix:?}");
        }
        Ok(())
    })
    .await
    .with_context(|| format!("health check timed out after {HEALTHCHECK_TIMEOUT:?}"))?
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

/// Returns `200` only after the watcher reaches its clean-poll admission threshold.
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
