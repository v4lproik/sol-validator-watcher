use super::{bind_http_listener, check_health, readiness_handler, run_http_server, AppState};
use crate::{
    metrics::Metrics,
    tasks::{TaskExit, TaskManager},
    watcher_state::{WatcherStateMailbox, ADMISSION_CLEAN_POLL_ROUNDS},
};
use anyhow::Result;
use axum::{extract::State, http::StatusCode, routing::get, Router};
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::test]
async fn readiness_handler_tracks_actor_admission() -> Result<()> {
    let mailbox = WatcherStateMailbox::new();
    let telemetry = mailbox.rpc_telemetry();
    let metrics = Arc::new(Metrics::new()?);
    let (watcher_state, actor) = mailbox.start("readiness-test", Arc::clone(&metrics));
    let app_state = AppState::new(metrics, watcher_state.clone());
    let mut tasks = TaskManager::new();
    tasks.spawn("watcher-state-actor", actor.run(tasks.shutdown_handle()));
    tasks.spawn("readiness-handler-scenario", async move {
        let (status, body) = readiness_handler(State(app_state.clone())).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            body,
            format!(
                "warming up: 0/{ADMISSION_CLEAN_POLL_ROUNDS} consecutive clean polling rounds"
            )
        );

        for _ in 0..ADMISSION_CLEAN_POLL_ROUNDS {
            watcher_state.begin_poll().await?;
            telemetry.record_response(200)?;
            watcher_state.complete_poll().await?;
        }

        let (status, body) = readiness_handler(State(app_state)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            format!(
                "ready: {ADMISSION_CLEAN_POLL_ROUNDS}/{ADMISSION_CLEAN_POLL_ROUNDS} consecutive clean polling rounds"
            )
        );
        Ok(TaskExit::ShutdownRequested)
    });

    tasks.run().await
}

#[tokio::test]
async fn production_server_serves_metrics_health_and_readiness() -> Result<()> {
    let mailbox = WatcherStateMailbox::new();
    let metrics = Arc::new(Metrics::new()?);
    metrics.set_watcher_up("http-test", true);
    let (watcher_state, actor) = mailbox.start("http-test", Arc::clone(&metrics));
    let listener = bind_http_listener("127.0.0.1:0".parse()?).await?;
    let address = listener.local_addr()?;
    let app_state = AppState::new(metrics, watcher_state);
    let mut tasks = TaskManager::new();
    tasks.spawn("watcher-state-actor", actor.run(tasks.shutdown_handle()));
    tasks.spawn(
        "metrics-http-server",
        run_http_server(app_state, listener, tasks.shutdown_handle()),
    );
    tasks.spawn("http-boundary-scenario", async move {
        check_health(address).await?;
        let client = reqwest::Client::new();
        let metrics_response = client
            .get(format!("http://{address}/metrics"))
            .send()
            .await?
            .error_for_status()?;
        assert!(metrics_response
            .text()
            .await?
            .contains("sol_watcher_up{network=\"http-test\"} 1"));

        let readiness_response = client
            .get(format!("http://{address}/readyz"))
            .send()
            .await?;
        assert_eq!(readiness_response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            readiness_response.text().await?,
            format!("warming up: 0/{ADMISSION_CLEAN_POLL_ROUNDS} consecutive clean polling rounds")
        );
        Ok(TaskExit::ShutdownRequested)
    });

    tasks.run().await
}

#[tokio::test]
async fn healthcheck_accepts_liveness_and_rejects_unavailable_status() -> Result<()> {
    let healthy_listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let healthy_address = healthy_listener.local_addr()?;
    let unavailable_listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let unavailable_address = unavailable_listener.local_addr()?;
    let mut tasks = TaskManager::new();
    let mut healthy_shutdown = tasks.shutdown_handle();
    tasks.spawn("healthy-http-server", async move {
        let app = Router::new().route("/healthz", get(|| async { StatusCode::OK }));
        axum::serve(healthy_listener, app)
            .with_graceful_shutdown(async move {
                healthy_shutdown.cancelled().await;
            })
            .await?;
        Ok(TaskExit::Finished)
    });
    let mut unavailable_shutdown = tasks.shutdown_handle();
    tasks.spawn("unavailable-http-server", async move {
        let app = Router::new().route(
            "/healthz",
            get(|| async { StatusCode::SERVICE_UNAVAILABLE }),
        );
        axum::serve(unavailable_listener, app)
            .with_graceful_shutdown(async move {
                unavailable_shutdown.cancelled().await;
            })
            .await?;
        Ok(TaskExit::Finished)
    });
    tasks.spawn("healthcheck-scenario", async move {
        check_health(healthy_address).await?;
        let Err(error) = check_health(unavailable_address).await else {
            anyhow::bail!("a non-200 health response must fail the health check");
        };
        assert!(
            error
                .to_string()
                .contains("health endpoint returned unexpected status"),
            "{error:#}"
        );
        Ok(TaskExit::ShutdownRequested)
    });

    tasks.run().await
}
