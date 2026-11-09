use super::{AttemptDecision, AttemptKind, FailoverController, SourceChain};
use crate::{
    config::{Config, ConnectorConfig, HealthCommands, HealthModeConfig, ModesConfig},
    metrics::Metrics,
    mode::CommandId,
    tasks::{TaskExit, TaskManager},
    watcher_state::WatcherStateMailbox,
};
use anyhow::{Context, Result};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_tungstenite::accept_async;

#[tokio::test]
async fn failover_gate_recovers_only_after_the_failed_command_succeeds() {
    let failovers = FailoverController::new(1, Duration::from_secs(60));
    let failed_command = CommandId::EpochInfo;
    let related_command = CommandId::VoteAccounts;
    let started_at = Instant::now();
    let retry_at = started_at + Duration::from_secs(60);

    assert_eq!(
        failovers
            .begin_attempt_at(0, failed_command, started_at)
            .await,
        AttemptDecision::Attempt(AttemptKind::Normal(0))
    );
    assert_eq!(
        failovers
            .record_failure_at(0, failed_command, AttemptKind::Normal(0), started_at)
            .await,
        "activated"
    );
    assert_eq!(
        failovers
            .begin_attempt_at(0, failed_command, retry_at - Duration::from_millis(1))
            .await,
        AttemptDecision::Skip
    );
    assert_eq!(
        failovers
            .begin_attempt_at(0, related_command, retry_at)
            .await,
        AttemptDecision::Skip
    );
    assert_eq!(
        failovers
            .begin_attempt_at(0, failed_command, retry_at)
            .await,
        AttemptDecision::Attempt(AttemptKind::RecoveryProbe(0))
    );
    assert_eq!(
        failovers
            .record_failure_at(0, related_command, AttemptKind::Normal(0), retry_at)
            .await,
        "already-active"
    );
    assert_eq!(
        failovers
            .begin_attempt_at(0, failed_command, retry_at)
            .await,
        AttemptDecision::Skip
    );
    assert!(
        failovers
            .record_success(0, failed_command, AttemptKind::RecoveryProbe(0))
            .await
    );
    assert_eq!(
        failovers
            .begin_attempt_at(0, related_command, retry_at)
            .await,
        AttemptDecision::Attempt(AttemptKind::Normal(1))
    );
}

#[tokio::test]
async fn failed_recovery_attempt_restarts_the_cooldown() {
    let failovers = FailoverController::new(1, Duration::from_secs(60));
    let command = CommandId::EpochInfo;
    let started_at = Instant::now();
    let first_retry = started_at + Duration::from_secs(60);
    let second_retry = started_at + Duration::from_secs(120);

    assert_eq!(
        failovers
            .record_failure_at(0, command, AttemptKind::Normal(0), started_at)
            .await,
        "activated"
    );
    assert_eq!(
        failovers.begin_attempt_at(0, command, first_retry).await,
        AttemptDecision::Attempt(AttemptKind::RecoveryProbe(0))
    );
    assert_eq!(
        failovers
            .record_failure_at(0, command, AttemptKind::RecoveryProbe(0), first_retry)
            .await,
        "recovery-probe-failed"
    );
    assert_eq!(
        failovers
            .begin_attempt_at(0, command, second_retry - Duration::from_millis(1),)
            .await,
        AttemptDecision::Skip
    );
    assert_eq!(
        failovers.begin_attempt_at(0, command, second_retry).await,
        AttemptDecision::Attempt(AttemptKind::RecoveryProbe(0))
    );
}

#[tokio::test]
async fn stale_failure_does_not_reopen_a_recovered_gate() {
    let failovers = FailoverController::new(1, Duration::ZERO);
    let command = CommandId::EpochInfo;
    let now = Instant::now();
    let stale_attempt = AttemptKind::Normal(0);
    assert_eq!(
        failovers.begin_attempt_at(0, command, now).await,
        AttemptDecision::Attempt(stale_attempt)
    );
    assert_eq!(
        failovers.begin_attempt_at(0, command, now).await,
        AttemptDecision::Attempt(AttemptKind::Normal(0))
    );
    assert_eq!(
        failovers
            .record_failure_at(0, command, AttemptKind::Normal(0), now)
            .await,
        "activated"
    );
    let recovery_attempt = AttemptKind::RecoveryProbe(0);
    assert_eq!(
        failovers.begin_attempt_at(0, command, now).await,
        AttemptDecision::Attempt(recovery_attempt)
    );
    assert!(failovers.record_success(0, command, recovery_attempt).await);
    assert_eq!(
        failovers
            .record_failure_at(0, command, stale_attempt, now)
            .await,
        "stale-attempt"
    );
    assert_eq!(
        failovers.begin_attempt_at(0, command, now).await,
        AttemptDecision::Attempt(AttemptKind::Normal(1))
    );
}

#[tokio::test]
async fn websocket_fallback_counts_failed_connector_probe() -> Result<()> {
    let (rejected_url, rejected_server) = start_websocket_server(WebsocketResponse::Reject).await?;
    let (accepted_url, accepted_server) = start_websocket_server(WebsocketResponse::Accept).await?;
    let config = websocket_config(
        "websocket-fallback-test",
        vec![
            ConnectorConfig {
                name: "rejected-ws".to_string(),
                provider: "mock".to_string(),
                websocket: Some(crate::config::WebsocketTransportConfig::new(rejected_url)),
                ..ConnectorConfig::default()
            },
            ConnectorConfig {
                name: "accepted-ws".to_string(),
                provider: "mock".to_string(),
                websocket: Some(crate::config::WebsocketTransportConfig::new(accepted_url)),
                ..ConnectorConfig::default()
            },
        ],
        &["rejected-ws", "accepted-ws"],
    );
    let mailbox = WatcherStateMailbox::new();
    let sources = SourceChain::from_config(&config, mailbox.rpc_telemetry())?;
    let metrics = Arc::new(Metrics::new()?);
    let (state, actor) = mailbox.start(&config.network, Arc::clone(&metrics));
    metrics.begin_poll(&config, &sources);
    let mut tasks = TaskManager::new();
    tasks.spawn("watcher-state-actor", actor.run(tasks.shutdown_handle()));
    tasks.spawn("websocket-fallback-scenario", async move {
        state.begin_poll().await?;
        let source = sources
            .websocket_healthy()
            .await?
            .context("expected the fallback websocket connector to succeed")?;
        assert_eq!(source.connector.as_ref(), "accepted-ws");
        assert_eq!(source.precedence, 2);

        let completion = state.complete_poll().await?;
        assert_eq!(completion.state.clean_poll_rounds, 1);

        let rendered = metrics.render()?;
        assert!(rendered.contains(
            "sol_ws_probe_failures_total{connector=\"rejected-ws\",network=\"websocket-fallback-test\"} 1"
        ));
        assert!(rendered.contains(
            "sol_ws_probe_failures_total{connector=\"accepted-ws\",network=\"websocket-fallback-test\"} 0"
        ));

        rejected_server
            .await
            .context("rejected websocket server task failed")??;
        accepted_server
            .await
            .context("accepted websocket server task failed")??;

        Ok(TaskExit::ShutdownRequested)
    });

    tasks.run().await
}

#[tokio::test]
async fn websocket_route_exhaustion_preserves_every_connector_failure() -> Result<()> {
    let (first_url, first_server) = start_websocket_server(WebsocketResponse::Reject).await?;
    let (second_url, second_server) = start_websocket_server(WebsocketResponse::Reject).await?;
    let config = websocket_config(
        "websocket-exhaustion-test",
        vec![
            ConnectorConfig {
                name: "first-ws".to_string(),
                provider: "first-provider".to_string(),
                websocket: Some(crate::config::WebsocketTransportConfig::new(first_url)),
                ..ConnectorConfig::default()
            },
            ConnectorConfig {
                name: "second-ws".to_string(),
                provider: "second-provider".to_string(),
                websocket: Some(crate::config::WebsocketTransportConfig::new(second_url)),
                ..ConnectorConfig::default()
            },
        ],
        &["first-ws", "second-ws"],
    );
    let sources =
        SourceChain::from_config(&config, crate::watcher_state::RpcTelemetry::unobserved())?;

    let error = match sources.websocket_healthy().await {
        Ok(_) => anyhow::bail!("all rejected websocket connectors must fail"),
        Err(error) => error,
    };
    first_server
        .await
        .context("first rejected websocket server task failed")??;
    second_server
        .await
        .context("second rejected websocket server task failed")??;

    let message = format!("{error:#}");
    assert!(message.contains("connector route exhausted for websocket health command"));
    assert!(message.contains("connector \"first-ws\" (provider \"first-provider\")"));
    assert!(message.contains("connector \"second-ws\" (provider \"second-provider\")"));
    Ok(())
}

fn websocket_config(network: &str, connectors: Vec<ConnectorConfig>, route: &[&str]) -> Config {
    Config {
        network: network.to_string(),
        connectors,
        modes: ModesConfig {
            health: HealthModeConfig {
                commands: HealthCommands {
                    rpc_health: Vec::new(),
                    ws_health: route.iter().map(|name| (*name).to_string()).collect(),
                    node_status: Vec::new(),
                },
            },
            ..ModesConfig::default()
        },
        ..Config::default()
    }
}

#[derive(Clone, Copy)]
enum WebsocketResponse {
    Accept,
    Reject,
}

async fn start_websocket_server(
    response: WebsocketResponse,
) -> Result<(String, JoinHandle<Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind websocket test server")?;
    let address = listener
        .local_addr()
        .context("failed to read websocket test server address")?;
    let task = tokio::spawn(async move {
        let (stream, _) = listener
            .accept()
            .await
            .context("failed to accept websocket test connection")?;
        if matches!(response, WebsocketResponse::Accept) {
            let stream = accept_async(stream)
                .await
                .context("failed to accept websocket test handshake")?;
            drop(stream);
        }
        Ok(())
    });

    Ok((format!("ws://{address}"), task))
}
