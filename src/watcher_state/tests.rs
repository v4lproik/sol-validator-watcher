use super::{RpcResponseCounts, WatcherStateMailbox, ADMISSION_SUCCESSFUL_POLL_ROUNDS};
use crate::{
    failover::{
        FailoverKey, FailoverScope, FailoverStatus, FailoverTransition, FailoverTransitionKind,
    },
    metrics::Metrics,
    mode::{CommandId, ModeSourceRequirement},
    tasks::{TaskExit, TaskManager},
};
use anyhow::Result;
use std::sync::Arc;

#[test]
fn response_counts_classify_bounded_statuses() {
    let mut responses = RpcResponseCounts::default();
    for status in [99, 200, 204, 307, 403, 429, 500, 502] {
        responses.record_response(status);
    }
    responses.record_failure();

    assert_eq!(
        responses,
        RpcResponseCounts {
            total: 8,
            successful_2xx: 2,
            redirection_3xx: 1,
            client_error_4xx: 2,
            server_error_5xx: 2,
            other: 1,
            status_200: 1,
            status_403: 1,
            status_429: 1,
            status_500: 1,
            failed_attempts: 1,
        }
    );
}

#[test]
fn response_count_deltas_are_fieldwise_and_saturating() {
    let current = RpcResponseCounts {
        total: 9,
        successful_2xx: 8,
        redirection_3xx: 7,
        client_error_4xx: 6,
        server_error_5xx: 5,
        other: 4,
        status_200: 3,
        status_403: 2,
        status_429: 1,
        status_500: 0,
        failed_attempts: 10,
    };
    let previous = RpcResponseCounts {
        total: 1,
        successful_2xx: 2,
        redirection_3xx: 3,
        client_error_4xx: 4,
        server_error_5xx: 5,
        other: 6,
        status_200: 7,
        status_403: 8,
        status_429: 9,
        status_500: 10,
        failed_attempts: 3,
    };

    assert_eq!(
        current.delta_since(previous),
        RpcResponseCounts {
            total: 8,
            successful_2xx: 6,
            redirection_3xx: 4,
            client_error_4xx: 2,
            server_error_5xx: 0,
            other: 0,
            status_200: 0,
            status_403: 0,
            status_429: 0,
            status_500: 0,
            failed_attempts: 7,
        }
    );
}

#[tokio::test]
async fn actor_orders_rpc_events_before_poll_completion_and_shuts_down_supervised() -> Result<()> {
    let mailbox = WatcherStateMailbox::new();
    let telemetry = mailbox.runtime_telemetry();
    let metrics = Arc::new(Metrics::new()?);
    let (state, actor) = mailbox.start("actor-test", &[], Arc::clone(&metrics));
    let mut tasks = TaskManager::new();
    tasks.spawn("watcher-state-actor", actor.run(tasks.shutdown_handle()));
    tasks.spawn("watcher-state-scenario", async move {
        for completed_rounds in 1..ADMISSION_SUCCESSFUL_POLL_ROUNDS {
            state.begin_poll().await?;
            telemetry.record_response(200)?;
            let completion = state.complete_poll().await?;
            assert!(!completion.admitted);
            assert_eq!(completion.state.successful_poll_rounds, completed_rounds);
            assert!(!completion.state.ready);
        }

        state.begin_poll().await?;
        telemetry.record_response(200)?;
        let completion = state.complete_poll().await?;
        assert!(completion.admitted);
        assert!(completion.state.ready);

        state.begin_poll().await?;
        let completion = state.fail_poll().await?;
        assert!(!completion.admitted);
        assert!(!completion.state.ready);
        assert_eq!(completion.state.successful_poll_rounds, 0);
        assert_eq!(
            completion.state.completed_poll_rounds,
            u64::from(ADMISSION_SUCCESSFUL_POLL_ROUNDS) + 1
        );
        assert_eq!(completion.state.rpc_responses.total, 5);

        state.begin_poll().await?;
        telemetry.record_response(429)?;
        telemetry
            .for_connector("actor-rpc")
            .record_failure("getHealth")?;
        let completion = state.complete_poll().await?;
        assert!(!completion.admitted);
        assert!(!completion.state.ready);
        assert_eq!(completion.state.successful_poll_rounds, 1);
        assert_eq!(completion.state.rpc_responses.total, 6);
        assert_eq!(completion.state.rpc_responses.successful_2xx, 5);
        assert_eq!(completion.state.rpc_responses.client_error_4xx, 1);
        assert_eq!(completion.state.rpc_responses.status_200, 5);
        assert_eq!(completion.state.rpc_responses.status_429, 1);
        assert_eq!(completion.state.rpc_responses.failed_attempts, 1);

        state.begin_poll().await?;
        telemetry.record_response(200)?;
        let completion = state.complete_poll().await?;
        assert_eq!(completion.state.successful_poll_rounds, 2);

        state.reset_admission("reloaded-network").await?;
        let snapshot = state.snapshot();
        assert_eq!(snapshot.network.as_ref(), "reloaded-network");
        assert_eq!(snapshot.completed_poll_rounds, 0);
        assert_eq!(snapshot.successful_poll_rounds, 0);
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

#[tokio::test]
async fn failover_degrades_ready_state_and_resets_only_with_source_runtime() -> Result<()> {
    let key = FailoverKey {
        connector: Arc::from("primary"),
        provider: Arc::from("mock"),
        source_requirement: ModeSourceRequirement::RpcExtended,
        scope: FailoverScope::Command,
        command: Some(CommandId::RecentPerformanceSamples),
    };
    let mailbox = WatcherStateMailbox::new();
    let telemetry = mailbox.runtime_telemetry();
    let metrics = Arc::new(Metrics::new()?);
    let (state, actor) = mailbox.start(
        "failover-state-test",
        std::slice::from_ref(&key),
        Arc::clone(&metrics),
    );
    let mut tasks = TaskManager::new();
    tasks.spawn("watcher-state-actor", actor.run(tasks.shutdown_handle()));
    tasks.spawn("failover-state-scenario", async move {
        for _ in 0..ADMISSION_SUCCESSFUL_POLL_ROUNDS {
            state.begin_poll().await?;
            state.complete_poll().await?;
        }
        assert!(state.snapshot().ready);

        telemetry.record_failover_transition(FailoverTransition {
            key: key.clone(),
            status: FailoverStatus::Open,
            kind: FailoverTransitionKind::Activated,
        })?;
        state.begin_poll().await?;
        let completion = state.complete_poll().await?;
        assert!(completion.state.ready);
        assert!(completion.state.degraded);
        assert_eq!(
            completion.state.successful_poll_rounds,
            ADMISSION_SUCCESSFUL_POLL_ROUNDS
        );

        state.reset_admission("failover-state-test").await?;
        let preserved = state.snapshot();
        assert!(!preserved.ready);
        assert!(preserved.degraded);

        state
            .reset_runtime("replacement-network", std::slice::from_ref(&key))
            .await?;
        let reset = state.snapshot();
        assert_eq!(reset.network.as_ref(), "replacement-network");
        assert!(!reset.ready);
        assert!(!reset.degraded);

        let rendered = metrics.render()?;
        assert!(rendered.contains("sol_watcher_degraded{network=\"replacement-network\"} 0"));
        assert!(rendered.contains(
            "sol_source_failover_status{command=\"recent-performance-samples\",connector=\"primary\",network=\"replacement-network\",provider=\"mock\",scope=\"command\",source_requirement=\"rpc-extended\"} 0"
        ));
        assert!(rendered.contains(
            "sol_source_failover_transitions_total{command=\"recent-performance-samples\",connector=\"primary\",network=\"failover-state-test\",provider=\"mock\",scope=\"command\",source_requirement=\"rpc-extended\",transition=\"activated\"} 1"
        ));

        Ok(TaskExit::ShutdownRequested)
    });

    tasks.run().await
}
