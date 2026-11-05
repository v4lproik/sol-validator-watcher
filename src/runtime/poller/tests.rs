use super::{
    economics_capability_check_required, ensure_economics_capabilities_ready,
    mark_collection_modes_down, run_watcher_loop, run_watcher_loop_with_retry_policy,
    update_liveness_file, PollRetryPolicy, POLL_RETRY_POLICY,
};
use crate::{
    config::{
        Config, ConnectorConfig, ConsensusCommands, ConsensusModeConfig, EconomicsModeConfig,
        HealthCommands, HealthModeConfig, LeadershipCommands, LeadershipModeConfig, ModesConfig,
        WatchedValidatorConfig,
    },
    metrics::Metrics,
    mode::{ModeId, SolanaRpcCompatibility},
    runtime::{
        state::RuntimeState,
        test_support::{
            start_capability_probe_server, start_controlled_capability_probe_server,
            unique_test_dir,
        },
    },
    source::SourceChain,
    tasks::{TaskExit, TaskManager},
    watcher_state::{
        WatcherState, WatcherStateMailbox, WatcherStateSnapshot, ADMISSION_CLEAN_POLL_ROUNDS,
    },
};
use anyhow::{Context, Result};
use std::{
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

#[test]
fn update_liveness_file_writes_timestamp() -> Result<()> {
    let test_dir = unique_test_dir("liveness");
    fs::create_dir_all(&test_dir)
        .with_context(|| format!("failed to create {}", test_dir.display()))?;
    let path = test_dir.join("liveness.txt");
    update_liveness_file(Some(&path))?;

    let contents =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    assert!(!contents.trim().is_empty());

    fs::remove_dir_all(&test_dir)
        .with_context(|| format!("failed to remove directory {}", test_dir.display()))?;
    Ok(())
}

#[test]
fn poll_retry_delay_grows_exponentially_and_stays_bounded() {
    let delays = (1..=7)
        .map(|failed_rounds| POLL_RETRY_POLICY.delay(failed_rounds))
        .collect::<Vec<_>>();

    assert_eq!(delays, [1, 2, 4, 8, 16, 30, 30].map(Duration::from_secs));
    assert_eq!(POLL_RETRY_POLICY.delay(u32::MAX), Duration::from_secs(30));
}
#[test]
fn collection_failure_marks_enabled_collection_modes_down() -> Result<()> {
    let mut config = Config {
        network: "mainnet-beta".to_string(),
        ..Config::default()
    };
    config.modes.economics.enabled = true;
    let metrics = Metrics::new()?;
    for mode in ModeId::ALL {
        metrics.set_mode_up(&config.network, mode, true);
    }

    mark_collection_modes_down(&config, &metrics);

    let rendered = metrics.render()?;
    assert!(rendered.contains("sol_mode_up{mode=\"health\",network=\"mainnet-beta\"} 1"));
    assert!(rendered.contains("sol_mode_up{mode=\"consensus\",network=\"mainnet-beta\"} 0"));
    assert!(rendered.contains("sol_mode_up{mode=\"leadership\",network=\"mainnet-beta\"} 0"));
    assert!(rendered.contains("sol_mode_up{mode=\"economics\",network=\"mainnet-beta\"} 0"));
    Ok(())
}

#[test]
fn economics_capability_check_required_depends_on_mode_and_verification_flag() -> Result<()> {
    let mut runtime = disabled_economics_runtime()?;
    assert!(!economics_capability_check_required(&runtime));

    runtime.config.modes.economics = EconomicsModeConfig {
        enabled: true,
        validator: Some(WatchedValidatorConfig {
            identity: Some("11111111111111111111111111111111".to_string()),
            vote_account: Some("vote-1".to_string()),
            labels: vec![],
        }),
        ..EconomicsModeConfig::default()
    };
    assert!(economics_capability_check_required(&runtime));

    runtime.economics_capabilities_verified = true;
    assert!(!economics_capability_check_required(&runtime));
    Ok(())
}

#[tokio::test]
async fn ensure_economics_capabilities_ready_is_noop_when_disabled() -> Result<()> {
    let mut runtime = disabled_economics_runtime()?;

    ensure_economics_capabilities_ready(&mut runtime).await?;

    assert!(!runtime.economics_capabilities_verified);
    Ok(())
}

#[tokio::test]
async fn ensure_economics_capabilities_ready_skips_preverified_runtime() -> Result<()> {
    let mut runtime = disabled_economics_runtime()?;
    runtime.config.modes.economics = EconomicsModeConfig {
        enabled: true,
        validator: Some(WatchedValidatorConfig {
            identity: Some("11111111111111111111111111111111".to_string()),
            vote_account: Some("vote-1".to_string()),
            labels: vec![],
        }),
        ..EconomicsModeConfig::default()
    };
    runtime.economics_capabilities_verified = true;

    ensure_economics_capabilities_ready(&mut runtime).await?;

    assert!(runtime.economics_capabilities_verified);
    Ok(())
}

#[tokio::test]
async fn watcher_loop_fails_closed_before_publishing_capability_metrics() -> Result<()> {
    let (rpc_url, server_task) =
        start_capability_probe_server(Some("getRecentPerformanceSamples")).await?;
    let test_dir = unique_test_dir("startup-capability");
    fs::create_dir_all(&test_dir)
        .with_context(|| format!("failed to create {}", test_dir.display()))?;
    let config_path = test_dir.join("watcher.toml");
    let liveness_path = test_dir.join("watcher.liveness");
    let secret = "do-not-expose-startup-key";
    fs::write(
        &config_path,
        format!(
            r#"
network = "startup-test"
liveness_file = "{}"

[[connectors]]
name = "limited-rpc"
provider = "mock-provider"
features = []
solana_rpc_compatibility = "extended"
rpc = {{ url = "{rpc_url}/?api-key={secret}" }}

[modes.health.commands]
rpc_health = ["limited-rpc"]

[modes.consensus.commands]
epoch_info = ["limited-rpc"]
vote_accounts = ["limited-rpc"]
recent_performance_samples = ["limited-rpc"]

[modes.economics]
enabled = false
"#,
            liveness_path.display()
        ),
    )
    .with_context(|| format!("failed to write {}", config_path.display()))?;

    let mailbox = WatcherStateMailbox::new();
    let runtime = RuntimeState::load(config_path, mailbox.rpc_telemetry())?;
    let metrics = Arc::new(Metrics::new()?);
    let (watcher_state, watcher_state_actor) = mailbox.start("startup-test", Arc::clone(&metrics));
    let mut tasks = TaskManager::new();
    tasks.spawn(
        "watcher-state-actor",
        watcher_state_actor.run(tasks.shutdown_handle()),
    );
    tasks.spawn(
        "watcher-loop",
        run_watcher_loop(
            runtime,
            Arc::clone(&metrics),
            watcher_state,
            tasks.shutdown_handle(),
        ),
    );
    let Err(error) = tasks.run().await else {
        anyhow::bail!("unsupported startup connector must fail before collection");
    };
    let message = format!("{error:#}");
    assert!(message.contains("connector \"limited-rpc\""), "{message}");
    assert!(message.contains("getRecentPerformanceSamples"), "{message}");
    assert!(!message.contains(secret), "{message}");
    let rendered = metrics.render()?;
    assert!(
        !rendered.contains("sol_watcher_up{network=\"startup-test\""),
        "{rendered}"
    );
    assert!(
        !rendered.contains("sol_mode_command_route_info{"),
        "{rendered}"
    );
    assert!(!liveness_path.exists());

    fs::remove_dir_all(&test_dir)
        .with_context(|| format!("failed to remove directory {}", test_dir.display()))?;
    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn degraded_node_health_publishes_before_failed_poll_recovery() -> Result<()> {
    let (rpc_url, control, server_task) = start_controlled_capability_probe_server(None).await?;
    let test_dir = unique_test_dir("poll-recovery");
    fs::create_dir_all(&test_dir)
        .with_context(|| format!("failed to create {}", test_dir.display()))?;
    let liveness_path = test_dir.join("watcher.liveness");
    let config_path =
        write_poll_test_config(&test_dir, "poll-recovery-test", &rpc_url, &liveness_path)?;

    let mailbox = WatcherStateMailbox::new();
    let mut runtime = RuntimeState::load(config_path, mailbox.rpc_telemetry())?;
    runtime.connector_capabilities_verified = true;
    runtime.config.poll_interval_sec = 0;
    let metrics = Arc::new(Metrics::new()?);
    let (watcher_state, watcher_state_actor) =
        mailbox.start("poll-recovery-test", Arc::clone(&metrics));
    let observer_state = watcher_state.clone();
    let observer_metrics = Arc::clone(&metrics);
    let retry_policy = PollRetryPolicy {
        initial_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(4),
    };
    let mut tasks = TaskManager::new();
    tasks.spawn(
        "watcher-state-actor",
        watcher_state_actor.run(tasks.shutdown_handle()),
    );
    tasks.spawn(
        "watcher-loop",
        run_watcher_loop_with_retry_policy(
            runtime,
            Arc::clone(&metrics),
            watcher_state,
            tasks.shutdown_handle(),
            retry_policy,
        ),
    );
    tasks.spawn("poll-recovery-observer", async move {
        let admitted = wait_for_state(&observer_state, |state| state.ready).await?;
        assert_eq!(admitted.clean_poll_rounds, ADMISSION_CLEAN_POLL_ROUNDS);
        let rendered = observer_metrics.render()?;
        let successful_timestamp = metric_value(
            &rendered,
            "sol_last_successful_snapshot_timestamp_seconds{network=\"poll-recovery-test\"}",
        )?;

        control.set_health_behind(true);
        let degraded = wait_for_state(&observer_state, |state| {
            state.completed_poll_rounds > admitted.completed_poll_rounds && !state.ready
        })
        .await?;
        assert_eq!(degraded.clean_poll_rounds, 0);
        let rendered = observer_metrics.render()?;
        let degraded_timestamp = metric_value(
            &rendered,
            "sol_last_successful_snapshot_timestamp_seconds{network=\"poll-recovery-test\"}",
        )?;
        assert!(degraded_timestamp > successful_timestamp);
        assert_eq!(
            metric_value(
                &rendered,
                "sol_snapshot_failures_total{network=\"poll-recovery-test\"}",
            )?,
            0.0
        );
        assert!(rendered
            .contains("sol_node_health{identity=\"id-1\",network=\"poll-recovery-test\"} 0"));
        assert!(rendered.contains(
            "sol_node_catchup_distance_slots{identity=\"id-1\",network=\"poll-recovery-test\"} 42"
        ));
        assert!(rendered.contains("sol_rpc_up{network=\"poll-recovery-test\"} 0"));

        control.set_health_behind(false);
        let restored = wait_for_state(&observer_state, |state| state.ready).await?;
        let rendered = observer_metrics.render()?;
        let restored_timestamp = metric_value(
            &rendered,
            "sol_last_successful_snapshot_timestamp_seconds{network=\"poll-recovery-test\"}",
        )?;
        assert!(restored_timestamp >= degraded_timestamp);

        control.set_health_failure(true);
        let failed = wait_for_state(&observer_state, |state| {
            state.completed_poll_rounds > restored.completed_poll_rounds && !state.ready
        })
        .await?;
        assert_eq!(failed.clean_poll_rounds, 0);
        let rendered = observer_metrics.render()?;
        assert_eq!(
            metric_value(
                &rendered,
                "sol_last_successful_snapshot_timestamp_seconds{network=\"poll-recovery-test\"}",
            )?,
            restored_timestamp
        );
        assert!(
            metric_value(
                &rendered,
                "sol_snapshot_failures_total{network=\"poll-recovery-test\"}",
            )? >= 1.0
        );
        assert!(rendered.contains("sol_watcher_up{network=\"poll-recovery-test\"} 1"));
        assert!(rendered.contains("sol_watcher_ready{network=\"poll-recovery-test\"} 0"));

        control.set_health_failure(false);
        let readmitted = wait_for_state(&observer_state, |state| state.ready).await?;
        assert!(
            readmitted.completed_poll_rounds
                >= failed.completed_poll_rounds + u64::from(ADMISSION_CLEAN_POLL_ROUNDS)
        );

        Ok(TaskExit::ShutdownRequested)
    });

    tasks.run().await?;
    fs::remove_dir_all(&test_dir)
        .with_context(|| format!("failed to remove directory {}", test_dir.display()))?;
    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn liveness_write_failure_is_fatal_after_snapshot_publication() -> Result<()> {
    let (rpc_url, server_task) = start_capability_probe_server(None).await?;
    let test_dir = unique_test_dir("liveness-failure");
    fs::create_dir_all(&test_dir)
        .with_context(|| format!("failed to create {}", test_dir.display()))?;
    let invalid_parent = test_dir.join("not-a-directory");
    fs::write(&invalid_parent, "file")
        .with_context(|| format!("failed to write {}", invalid_parent.display()))?;
    let liveness_path = invalid_parent.join("watcher.liveness");
    let config_path =
        write_poll_test_config(&test_dir, "liveness-failure-test", &rpc_url, &liveness_path)?;

    let mailbox = WatcherStateMailbox::new();
    let mut runtime = RuntimeState::load(config_path, mailbox.rpc_telemetry())?;
    runtime.connector_capabilities_verified = true;
    let metrics = Arc::new(Metrics::new()?);
    let (watcher_state, watcher_state_actor) =
        mailbox.start("liveness-failure-test", Arc::clone(&metrics));
    let mut tasks = TaskManager::new();
    tasks.spawn(
        "watcher-state-actor",
        watcher_state_actor.run(tasks.shutdown_handle()),
    );
    tasks.spawn(
        "watcher-loop",
        run_watcher_loop(
            runtime,
            Arc::clone(&metrics),
            watcher_state,
            tasks.shutdown_handle(),
        ),
    );

    let Err(error) = tasks.run().await else {
        anyhow::bail!("an unwritable liveness path must terminate the watcher");
    };
    let message = format!("{error:#}");
    assert!(
        message.contains("failed to create liveness parent directory"),
        "{message}"
    );
    let rendered = metrics.render()?;
    assert!(rendered.contains(
        "sol_snapshot_duration_seconds_count{network=\"liveness-failure-test\",outcome=\"success\"} 1"
    ));
    assert!(
        metric_value(
            &rendered,
            "sol_last_successful_snapshot_timestamp_seconds{network=\"liveness-failure-test\"}",
        )? > 0.0
    );
    assert_eq!(
        metric_value(
            &rendered,
            "sol_snapshot_failures_total{network=\"liveness-failure-test\"}",
        )?,
        0.0
    );

    fs::remove_dir_all(&test_dir)
        .with_context(|| format!("failed to remove directory {}", test_dir.display()))?;
    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn watcher_loop_reports_state_actor_loss_without_shutdown() -> Result<()> {
    let mut runtime = disabled_economics_runtime()?;
    runtime.config.network = "actor-loss-test".to_string();
    runtime.connector_capabilities_verified = true;
    let mailbox = WatcherStateMailbox::new();
    let metrics = Arc::new(Metrics::new()?);
    let (watcher_state, watcher_state_actor) =
        mailbox.start("actor-loss-test", Arc::clone(&metrics));
    drop(watcher_state_actor);
    let tasks = TaskManager::new();

    let Err(error) =
        run_watcher_loop(runtime, metrics, watcher_state, tasks.shutdown_handle()).await
    else {
        anyhow::bail!("watcher-state actor loss must terminate the watcher");
    };

    assert!(
        format!("{error:#}").contains("failed to begin polling round"),
        "{error:#}"
    );
    Ok(())
}

#[tokio::test]
async fn watcher_loop_finishes_if_state_actor_is_unavailable_after_shutdown() -> Result<()> {
    let mut runtime = disabled_economics_runtime()?;
    runtime.config.network = "shutdown-race-test".to_string();
    runtime.connector_capabilities_verified = true;
    let mailbox = WatcherStateMailbox::new();
    let metrics = Arc::new(Metrics::new()?);
    let (watcher_state, watcher_state_actor) =
        mailbox.start("shutdown-race-test", Arc::clone(&metrics));
    drop(watcher_state_actor);

    let mut tasks = TaskManager::new();
    let mut watcher_shutdown = tasks.shutdown_handle();
    tasks.spawn("watcher-loop", async move {
        watcher_shutdown.cancelled().await;
        run_watcher_loop(runtime, metrics, watcher_state, watcher_shutdown).await
    });
    tasks.spawn("shutdown-trigger", async {
        Ok(TaskExit::ShutdownRequested)
    });

    tasks.run().await
}

#[tokio::test]
async fn watcher_loop_preserves_invalid_state_transition_during_shutdown() -> Result<()> {
    let mut runtime = disabled_economics_runtime()?;
    runtime.config.network = "shutdown-transition-test".to_string();
    runtime.connector_capabilities_verified = true;
    let mailbox = WatcherStateMailbox::new();
    let metrics = Arc::new(Metrics::new()?);
    let (watcher_state, watcher_state_actor) =
        mailbox.start("shutdown-transition-test", Arc::clone(&metrics));
    let actor_tasks = TaskManager::new();
    let actor_task = tokio::spawn(watcher_state_actor.run(actor_tasks.shutdown_handle()));
    watcher_state.begin_poll().await?;

    let mut shutdown_tasks = TaskManager::new();
    let watcher_shutdown = shutdown_tasks.shutdown_handle();
    shutdown_tasks.spawn("shutdown-trigger", async {
        Ok(TaskExit::ShutdownRequested)
    });
    shutdown_tasks.run().await?;

    let Err(error) = run_watcher_loop(runtime, metrics, watcher_state, watcher_shutdown).await
    else {
        anyhow::bail!("invalid state transitions must remain fatal during shutdown");
    };
    let actor_exit = actor_task
        .await
        .context("watcher-state actor task panicked")??;

    assert_eq!(actor_exit, TaskExit::Finished);
    assert!(
        format!("{error:#}").contains("a polling round is already active"),
        "{error:#}"
    );
    Ok(())
}

async fn wait_for_state(
    state: &WatcherState,
    predicate: impl Fn(&WatcherStateSnapshot) -> bool,
) -> Result<WatcherStateSnapshot> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let snapshot = state.snapshot();
            if predicate(&snapshot) {
                return snapshot;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .context("timed out waiting for watcher-state transition")
}

fn metric_value(rendered: &str, metric: &str) -> Result<f64> {
    let line = rendered
        .lines()
        .find(|line| line.starts_with(metric))
        .with_context(|| format!("missing metric sample {metric}"))?;
    line.split_whitespace()
        .next_back()
        .with_context(|| format!("missing value for metric sample {metric}"))?
        .parse()
        .with_context(|| format!("invalid value for metric sample {metric}"))
}

fn write_poll_test_config(
    test_dir: &Path,
    network: &str,
    rpc_url: &str,
    liveness_path: &Path,
) -> Result<PathBuf> {
    let config_path = test_dir.join("watcher.toml");
    fs::write(
        &config_path,
        format!(
            r#"
network = "{network}"
poll_interval_sec = 1
liveness_file = "{}"

[[connectors]]
name = "recovering-rpc"
provider = "mock-provider"
features = []
solana_rpc_compatibility = "core"
rpc = {{ url = "{rpc_url}" }}

[modes.health.commands]
rpc_health = ["recovering-rpc"]
node_status = ["recovering-rpc"]

[modes.consensus.commands]
epoch_info = ["recovering-rpc"]
vote_accounts = ["recovering-rpc"]
recent_performance_samples = []

[modes.leadership.commands]
current_block_production = []
current_leader_schedule = ["recovering-rpc"]
next_leader_schedule = ["recovering-rpc"]

[modes.economics]
enabled = false
"#,
            liveness_path.display()
        ),
    )
    .with_context(|| format!("failed to write {}", config_path.display()))?;
    Ok(config_path)
}

fn disabled_economics_runtime() -> Result<RuntimeState> {
    let config = Config {
        connectors: vec![ConnectorConfig {
            name: "primary".to_string(),
            provider: "solana".to_string(),
            features: Vec::new(),
            solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
            rpc: Some(crate::config::RpcTransportConfig::new(
                "https://api.mainnet-beta.solana.com".to_string(),
            )),
            ..ConnectorConfig::default()
        }],
        modes: modes_for("primary"),
        ..Config::default()
    };
    let rpc_telemetry = crate::watcher_state::RpcTelemetry::unobserved();
    let sources = SourceChain::from_config(&config, rpc_telemetry.clone())?;

    Ok(RuntimeState {
        config_path: PathBuf::from("config.toml"),
        config,
        sources,
        rpc_telemetry,
        config_modified: UNIX_EPOCH,
        metrics_address: SocketAddr::from(([127, 0, 0, 1], 8000)),
        connector_capabilities_verified: false,
        economics_capabilities_verified: false,
    })
}

fn modes_for(connector: &str) -> ModesConfig {
    let route = || vec![connector.to_string()];
    ModesConfig {
        health: HealthModeConfig {
            commands: HealthCommands {
                rpc_health: route(),
                ws_health: vec![],
                node_status: route(),
            },
        },
        consensus: ConsensusModeConfig {
            commands: ConsensusCommands {
                epoch_info: route(),
                vote_accounts: route(),
                recent_performance_samples: route(),
            },
        },
        leadership: LeadershipModeConfig {
            commands: LeadershipCommands {
                current_block_production: route(),
                current_leader_schedule: route(),
                next_leader_schedule: route(),
            },
        },
        economics: EconomicsModeConfig::default(),
    }
}
