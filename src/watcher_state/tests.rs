use super::{RpcResponseCounts, WatcherStateMailbox, ADMISSION_CLEAN_POLL_ROUNDS};
use crate::{
    metrics::Metrics,
    tasks::{TaskExit, TaskManager},
};
use anyhow::Result;
use std::sync::Arc;

#[test]
fn response_counts_classify_bounded_statuses() {
    let mut responses = RpcResponseCounts::default();
    for status in [200, 204, 307, 403, 429, 500, 502] {
        responses.record_response(status);
    }
    responses.record_failure();

    assert_eq!(
        responses,
        RpcResponseCounts {
            total: 7,
            successful_2xx: 2,
            redirection_3xx: 1,
            client_error_4xx: 2,
            server_error_5xx: 2,
            status_200: 1,
            status_403: 1,
            status_429: 1,
            status_500: 1,
            failed_attempts: 1,
            ..RpcResponseCounts::default()
        }
    );
}

#[tokio::test]
async fn actor_orders_rpc_events_before_poll_completion_and_shuts_down_supervised() -> Result<()> {
    let mailbox = WatcherStateMailbox::new();
    let telemetry = mailbox.rpc_telemetry();
    let metrics = Arc::new(Metrics::new()?);
    let (state, actor) = mailbox.start("actor-test", Arc::clone(&metrics));
    let mut tasks = TaskManager::new();
    tasks.spawn("watcher-state-actor", actor.run(tasks.shutdown_handle()));
    tasks.spawn("watcher-state-scenario", async move {
        for completed_rounds in 1..ADMISSION_CLEAN_POLL_ROUNDS {
            state.begin_poll().await?;
            telemetry.record_response(200)?;
            let completion = state.complete_poll().await?;
            assert!(!completion.admitted);
            assert_eq!(completion.state.clean_poll_rounds, completed_rounds);
            assert!(!completion.state.ready);
        }

        state.begin_poll().await?;
        telemetry.record_response(200)?;
        let completion = state.complete_poll().await?;
        assert!(completion.admitted);
        assert!(completion.state.ready);

        state.begin_poll().await?;
        telemetry.record_response(429)?;
        telemetry.record_failure()?;
        let completion = state.complete_poll().await?;
        assert!(!completion.admitted);
        assert!(!completion.state.ready);
        assert_eq!(completion.state.clean_poll_rounds, 0);
        assert_eq!(completion.state.rpc_responses.total, 6);
        assert_eq!(completion.state.rpc_responses.successful_2xx, 5);
        assert_eq!(completion.state.rpc_responses.client_error_4xx, 1);
        assert_eq!(completion.state.rpc_responses.status_200, 5);
        assert_eq!(completion.state.rpc_responses.status_429, 1);
        assert_eq!(completion.state.rpc_responses.failed_attempts, 1);

        state.begin_poll().await?;
        telemetry.record_response(200)?;
        let completion = state.complete_poll().await?;
        assert_eq!(completion.state.clean_poll_rounds, 1);

        state.reset_admission("reloaded-network").await?;
        let snapshot = state.snapshot();
        assert_eq!(snapshot.network.as_ref(), "reloaded-network");
        assert_eq!(snapshot.completed_poll_rounds, 0);
        assert_eq!(snapshot.clean_poll_rounds, 0);
        assert!(!snapshot.ready);
        assert_eq!(snapshot.rpc_responses.total, 7);

        let rendered = metrics.render()?;
        assert!(rendered.contains("sol_watcher_ready{network=\"reloaded-network\"} 0"));
        assert!(rendered.contains(
            "sol_rpc_http_response_status_count{network=\"reloaded-network\",status_code=\"429\"} 1"
        ));
        assert!(rendered.contains("sol_rpc_request_failure_count{network=\"reloaded-network\"} 1"));

        Ok(TaskExit::ShutdownRequested)
    });

    tasks.run().await
}
