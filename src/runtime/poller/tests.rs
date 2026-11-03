use super::{
    economics_capability_check_required, ensure_economics_capabilities_ready,
    mark_collection_modes_down, run_watcher_loop, update_liveness_file,
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
        test_support::{start_capability_probe_server, unique_test_dir},
    },
    source::SourceChain,
    tasks::TaskManager,
    watcher_state::WatcherStateMailbox,
};
use anyhow::{Context, Result};
use std::{fs, path::PathBuf, sync::Arc, time::UNIX_EPOCH};

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
rpc_url = "{rpc_url}/?api-key={secret}"

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

fn disabled_economics_runtime() -> Result<RuntimeState> {
    let config = Config {
        connectors: vec![ConnectorConfig {
            name: "primary".to_string(),
            provider: "solana".to_string(),
            features: Vec::new(),
            solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
            rpc_url: Some("https://api.mainnet-beta.solana.com".to_string()),
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
        metrics_port: 8000,
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
