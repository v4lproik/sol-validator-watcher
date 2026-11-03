use super::{readiness_handler, AppState};
use crate::{
    metrics::Metrics,
    tasks::{TaskExit, TaskManager},
    watcher_state::{WatcherStateMailbox, ADMISSION_CLEAN_POLL_ROUNDS},
};
use anyhow::Result;
use axum::{extract::State, http::StatusCode};
use std::sync::Arc;

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
