use super::*;
use crate::collector::Snapshot;

#[tokio::test]
async fn rpc_client_collect_snapshot_and_validate_economics_against_mock_rpc() -> Result<()> {
    let (rpc_url, server_task) = start_mock_rpc_server().await?;
    let mut config = Config {
        network: "devnet".to_string(),
        connectors: vec![ConnectorConfig {
            name: "primary".to_string(),
            provider: "mock-solana".to_string(),
            features: vec![
                ProviderFeature::HistoricalData,
                ProviderFeature::BlockFeeRewards,
            ],
            rpc: Some(crate::config::RpcTransportConfig::new(rpc_url.clone())),
            ..ConnectorConfig::default()
        }],
        modes: modes_for(&["primary"], true),
        watched_validators: vec![WatchedValidatorConfig {
            identity: Some("id-1".to_string()),
            vote_account: Some("vote-1".to_string()),
            labels: vec!["operator:test".to_string()],
        }],
        ..Config::default()
    };
    config.modes.economics.validator = Some(WatchedValidatorConfig {
        identity: Some("id-1".to_string()),
        vote_account: Some("vote-1".to_string()),
        labels: vec!["focus:economics".to_string()],
    });
    config.modes.economics.commands.recent_block_fee_rewards = vec!["primary".to_string()];
    let client = RpcClient::try_new(
        &rpc_transport(&rpc_url, 5),
        crate::RuntimeTelemetry::unobserved().for_connector("mock-rpc"),
    )?;

    assert!(client.node_health().await?.is_healthy());
    let sources = SourceChain::from_config(&config, crate::RuntimeTelemetry::unobserved())?;
    ensure_economics_supported(&sources, &config).await?;
    let snapshot = collect_snapshot(&sources, &config).await?;

    server_task.abort();

    assert_eq!(snapshot.network, "devnet");
    assert_eq!(snapshot.epoch, 9);
    assert_eq!(snapshot.slot, 810);
    assert_eq!(snapshot.epoch_slots, 100);
    let node = snapshot
        .node
        .as_ref()
        .ok_or_else(|| anyhow!("missing node-local metrics"))?;
    assert_eq!(node.identity, "id-1");
    assert_eq!(node.version, "3.1.8");
    assert_eq!(node.feature_set, Some(2_891_131_721));
    assert_eq!(node.block_height, 1_233);
    assert_eq!(node.health, crate::collector::NodeHealth::Healthy);
    assert_eq!(
        node.voting_status,
        crate::collector::NodeVotingStatus::Current
    );
    assert!(snapshot
        .data_sources
        .iter()
        .any(|source| source.command == CommandId::NodeStatus));
    assert!(snapshot
        .data_sources
        .iter()
        .any(|source| source.command == CommandId::ClusterNodes));
    assert_eq!(
        snapshot.watched_validators[0].gossip,
        GossipObservation::Present {
            version: Some("3.1.8".to_string()),
            feature_set: Some(2_891_131_721),
            shred_version: Some(56_123),
        }
    );
    assert_eq!(snapshot.vote_credits_sample_epoch, Some(8));
    assert_eq!(snapshot.reward_sample_epoch, Some(8));
    assert_eq!(snapshot.watched_validators.len(), 1);
    assert_eq!(
        snapshot.watched_validators[0].latest_epoch_reward_lamports,
        Some(12_000_000_000)
    );
    assert_eq!(
        snapshot.watched_validators[0].latest_epoch_rewarded_blocks_produced,
        3
    );
    assert_eq!(snapshot.watched_validators[0].blocks_produced, 4);
    assert_eq!(snapshot.watched_validators[0].future_leader_slots, 3);
    assert_eq!(
        snapshot.watched_validators[0].current_epoch_scheduled_leader_slots,
        Some(4)
    );
    assert_eq!(
        snapshot.watched_validators[0].upcoming_leader_slots,
        vec![810, 901, 950]
    );

    assert_eq!(snapshot.epoch_progress_pct, 10.0);
    assert_eq!(snapshot.epoch_slots_remaining, 90);
    assert_eq!(
        snapshot.cluster_performance,
        Some(ClusterPerformanceMetrics {
            average_slot_time_seconds: Some(0.4),
            transactions_per_second: 25.0,
            vote_transactions_per_second: Some(22.5),
            non_vote_transactions_per_second: Some(2.5),
            slots_per_second: 2.5,
        })
    );
    assert!(snapshot.estimated_epoch_end_unixtime.is_some());
    assert_eq!(snapshot.watched_validators[0].next_leader_slot, Some(810));
    assert_eq!(
        snapshot.watched_validators[0].slots_until_next_leader,
        Some(0)
    );
    assert_eq!(
        snapshot.watched_validators[0].seconds_until_next_leader,
        Some(0.0)
    );
    let funding = snapshot.watched_validators[0]
        .funding
        .context("missing watched-validator funding metrics")?;
    assert_eq!(funding.identity_balance_lamports, 3_000_000_000);
    assert_eq!(funding.vote_account_balance_lamports, 2_500_000_000);
    assert_eq!(
        funding.vote_account_rent_exempt_minimum_lamports,
        Some(900_000_000)
    );
    assert_eq!(funding.vat_reserve_lamports, Some(1_600_000_000));
    assert_eq!(funding.vat_epochs_available, Some(2));
    assert_eq!(funding.vat_next_epoch_funded, Some(true));
    let economics = snapshot
        .economics
        .as_ref()
        .context("missing economics metrics")?;
    assert_eq!(economics.identity_balance_lamports, 3_000_000_000);
    assert_eq!(economics.vote_account_balance_lamports, 2_500_000_000);
    assert_eq!(
        economics.vote_account_rent_exempt_minimum_lamports,
        Some(900_000_000)
    );
    assert_eq!(economics.vat_reserve_lamports, Some(1_600_000_000));
    assert_eq!(economics.vat_epochs_available, Some(2));
    assert_eq!(economics.vat_next_epoch_funded, Some(true));
    assert_eq!(
        economics.recent_block_fee_rewards,
        Some(RecentBlockFeeRewards {
            latest_slot: 805,
            latest_reward_lamports: 3_000_000,
            total_reward_lamports: 4_000_000,
            sampled_block_count: 2,
        })
    );
    assert!(snapshot.data_sources.iter().any(|source| {
        source.command == CommandId::RecentBlockFeeRewards && source.connector.as_ref() == "primary"
    }));
    Ok(())
}

#[tokio::test]
async fn gossip_observation_distinguishes_missing_identity_from_unavailable_route() -> Result<()> {
    let (missing_url, missing_task) =
        start_mock_rpc_server_with_profile(MockRpcProfile::MissingWatchedClusterNode).await?;
    let missing_config = Config {
        network: "devnet".to_string(),
        connectors: vec![ConnectorConfig {
            name: "primary".to_string(),
            provider: "mock-solana".to_string(),
            features: Vec::new(),
            rpc: Some(crate::config::RpcTransportConfig::new(missing_url)),
            ..ConnectorConfig::default()
        }],
        modes: modes_for(&["primary"], false),
        watched_validators: vec![WatchedValidatorConfig {
            identity: Some("id-1".to_string()),
            vote_account: Some("vote-1".to_string()),
            labels: Vec::new(),
        }],
        ..Config::default()
    };
    let missing_sources =
        SourceChain::from_config(&missing_config, crate::RuntimeTelemetry::unobserved())?;
    let missing_snapshot = collect_snapshot(&missing_sources, &missing_config).await?;
    missing_task.abort();
    assert_eq!(
        missing_snapshot.watched_validators[0].gossip,
        GossipObservation::Missing
    );

    let (unavailable_url, unavailable_task) = start_mock_rpc_server().await?;
    let mut unavailable_config = Config {
        network: "devnet".to_string(),
        connectors: vec![ConnectorConfig {
            name: "primary".to_string(),
            provider: "mock-solana".to_string(),
            features: Vec::new(),
            rpc: Some(crate::config::RpcTransportConfig::new(unavailable_url)),
            ..ConnectorConfig::default()
        }],
        modes: modes_for(&["primary"], false),
        watched_validators: vec![WatchedValidatorConfig {
            identity: Some("id-1".to_string()),
            vote_account: Some("vote-1".to_string()),
            labels: Vec::new(),
        }],
        ..Config::default()
    };
    unavailable_config
        .modes
        .consensus
        .commands
        .cluster_nodes
        .clear();
    let unavailable_sources =
        SourceChain::from_config(&unavailable_config, crate::RuntimeTelemetry::unobserved())?;
    let unavailable_snapshot = collect_snapshot(&unavailable_sources, &unavailable_config).await?;
    unavailable_task.abort();
    assert_eq!(
        unavailable_snapshot.watched_validators[0].gossip,
        GossipObservation::Unavailable
    );
    assert!(!unavailable_snapshot
        .data_sources
        .iter()
        .any(|source| source.command == CommandId::ClusterNodes));
    Ok(())
}

#[tokio::test]
async fn missing_performance_samples_omit_time_estimates_without_failing_collection() -> Result<()>
{
    let (rpc_url, server_task) =
        start_mock_rpc_server_with_profile(MockRpcProfile::NoPerformanceSamples).await?;
    let config = Config {
        network: "devnet".to_string(),
        connectors: vec![ConnectorConfig {
            name: "primary".to_string(),
            provider: "mock-solana".to_string(),
            features: Vec::new(),
            rpc: Some(crate::config::RpcTransportConfig::new(rpc_url)),
            ..ConnectorConfig::default()
        }],
        modes: modes_for(&["primary"], false),
        watched_validators: vec![WatchedValidatorConfig {
            identity: Some("id-1".to_string()),
            vote_account: Some("vote-1".to_string()),
            labels: vec!["operator:test".to_string()],
        }],
        ..Config::default()
    };
    let sources = SourceChain::from_config(&config, crate::RuntimeTelemetry::unobserved())?;
    let snapshot = collect_snapshot(&sources, &config).await?;
    server_task.abort();

    assert_eq!(snapshot.cluster_performance, None);
    assert_eq!(snapshot.estimated_epoch_end_unixtime, None);
    assert_eq!(snapshot.watched_validators[0].next_leader_slot, Some(810));
    assert_eq!(
        snapshot.watched_validators[0].seconds_until_next_leader,
        None
    );
    assert!(!snapshot
        .data_sources
        .iter()
        .any(|source| source.command == CommandId::RecentPerformanceSamples));
    Ok(())
}

#[tokio::test]
async fn protocol_failure_activates_only_command_failover() -> Result<()> {
    let (primary_rpc_url, primary_server_task, primary_methods) =
        start_recording_mock_rpc_server(MockRpcProfile::InvalidPerformanceTransactionSplit).await?;
    let (fallback_rpc_url, fallback_server_task, fallback_methods) =
        start_recording_mock_rpc_server(MockRpcProfile::Default).await?;
    let (config, test_dir) = load_extended_backup_config(&primary_rpc_url, &fallback_rpc_url)?;
    let sources = SourceChain::from_config(&config, crate::RuntimeTelemetry::unobserved())?;

    let first = collect_snapshot(&sources, &config).await?;
    let second = collect_snapshot(&sources, &config).await?;

    assert_eq!(
        selected_connector(&first, CommandId::RecentPerformanceSamples),
        Some("fallback")
    );
    assert_eq!(
        selected_connector(&second, CommandId::RecentPerformanceSamples),
        Some("fallback")
    );
    assert_eq!(
        selected_connector(&first, CommandId::CurrentBlockProduction),
        Some("primary")
    );
    assert_eq!(
        selected_connector(&second, CommandId::CurrentBlockProduction),
        Some("primary")
    );
    assert_eq!(
        recorded_method_count(&primary_methods, "getRecentPerformanceSamples").await,
        1
    );
    assert_eq!(
        recorded_method_count(&fallback_methods, "getRecentPerformanceSamples").await,
        2
    );
    assert_eq!(
        recorded_method_count(&primary_methods, "getBlockProduction").await,
        2
    );
    assert_eq!(
        recorded_method_count(&fallback_methods, "getBlockProduction").await,
        0
    );
    assert_eq!(
        recorded_method_count(&primary_methods, "getEpochInfo").await,
        2
    );
    assert_eq!(
        recorded_method_count(&fallback_methods, "getEpochInfo").await,
        0
    );

    primary_server_task.abort();
    fallback_server_task.abort();
    std::fs::remove_dir_all(&test_dir)?;
    Ok(())
}

#[tokio::test]
async fn transport_failure_activates_source_requirement_failover() -> Result<()> {
    let primary_listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let primary_rpc_url = format!("http://{}", primary_listener.local_addr()?);
    drop(primary_listener);
    let (fallback_rpc_url, fallback_server_task, fallback_methods) =
        start_recording_mock_rpc_server(MockRpcProfile::Default).await?;
    let (config, test_dir) = load_extended_backup_config(&primary_rpc_url, &fallback_rpc_url)?;
    let sources = SourceChain::from_config(&config, crate::RuntimeTelemetry::unobserved())?;

    let failed_command = sources
        .optional(&crate::collector::command::GetRecentPerformanceSamples)
        .await?
        .context("fallback should satisfy the failed transport command")?;
    assert_eq!(failed_command.source.connector.as_ref(), "fallback");

    let related_command = sources
        .optional(&crate::collector::command::GetCurrentBlockProduction)
        .await?
        .context("related command should use the shared requirement fallback")?;
    assert_eq!(related_command.source.connector.as_ref(), "fallback");
    assert_eq!(
        recorded_method_count(&fallback_methods, "getRecentPerformanceSamples").await,
        1
    );
    assert_eq!(
        recorded_method_count(&fallback_methods, "getBlockProduction").await,
        1
    );

    fallback_server_task.abort();
    std::fs::remove_dir_all(&test_dir)?;
    Ok(())
}

#[tokio::test]
async fn command_failure_recovers_after_failover_cooldown() -> Result<()> {
    let (primary_rpc_url, primary_server_task, primary_methods) =
        start_recording_mock_rpc_server(MockRpcProfile::InvalidPerformanceTransactionSplitOnce)
            .await?;
    let (fallback_rpc_url, fallback_server_task, fallback_methods) =
        start_recording_mock_rpc_server(MockRpcProfile::Default).await?;
    let (config, test_dir) = load_extended_backup_config(&primary_rpc_url, &fallback_rpc_url)?;
    let sources = SourceChain::from_config(&config, crate::RuntimeTelemetry::unobserved())?
        .with_failover_cooldown(std::time::Duration::ZERO);

    let failed_command = sources
        .optional(&crate::collector::command::GetRecentPerformanceSamples)
        .await?
        .context("fallback performance command should succeed")?;
    assert_eq!(failed_command.source.connector.as_ref(), "fallback");

    let related_command = sources
        .optional(&crate::collector::command::GetCurrentBlockProduction)
        .await?
        .context("unrelated command should remain on the primary")?;
    assert_eq!(related_command.source.connector.as_ref(), "primary");

    let recovery = sources
        .optional(&crate::collector::command::GetRecentPerformanceSamples)
        .await?
        .context("failed command should recover on the primary")?;
    assert_eq!(recovery.source.connector.as_ref(), "primary");

    let restored = sources
        .optional(&crate::collector::command::GetCurrentBlockProduction)
        .await?
        .context("related command should return to the recovered primary")?;
    assert_eq!(restored.source.connector.as_ref(), "primary");

    assert_eq!(
        recorded_method_count(&primary_methods, "getRecentPerformanceSamples").await,
        2
    );
    assert_eq!(
        recorded_method_count(&fallback_methods, "getRecentPerformanceSamples").await,
        1
    );
    assert_eq!(
        recorded_method_count(&primary_methods, "getBlockProduction").await,
        2
    );
    assert_eq!(
        recorded_method_count(&fallback_methods, "getBlockProduction").await,
        0
    );

    primary_server_task.abort();
    fallback_server_task.abort();
    std::fs::remove_dir_all(&test_dir)?;
    Ok(())
}

static SOURCE_FAILOVER_TEST_DIR_SEQUENCE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

fn load_extended_backup_config(
    primary_rpc_url: &str,
    fallback_rpc_url: &str,
) -> Result<(Config, std::path::PathBuf)> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let sequence =
        SOURCE_FAILOVER_TEST_DIR_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let test_dir = std::env::temp_dir().join(format!(
        "sol-validator-watcher-source-failover-{nonce}-{sequence}"
    ));
    std::fs::create_dir_all(&test_dir)?;
    let config_path = test_dir.join("watcher.toml");
    std::fs::write(
        &config_path,
        format!(
            r#"
network = "devnet"

[[connectors]]
name = "primary"
provider = "primary-provider"
features = []
solana_rpc_compatibility = "extended"
rpc = {{ url = "{primary_rpc_url}" }}

[connectors.backups]
rpc-extended = ["fallback"]

[[connectors]]
name = "fallback"
provider = "fallback-provider"
features = []
solana_rpc_compatibility = "extended"
rpc = {{ url = "{fallback_rpc_url}" }}

[modes.health.commands]
rpc_health = ["primary"]

[modes.consensus.commands]
epoch_info = ["primary"]
vote_accounts = ["primary"]
recent_performance_samples = ["primary"]

[modes.leadership.commands]
current_block_production = ["primary"]
"#
        ),
    )?;
    Ok((Config::load(&config_path)?, test_dir))
}

fn selected_connector(snapshot: &Snapshot, command: CommandId) -> Option<&str> {
    snapshot
        .data_sources
        .iter()
        .find(|source| source.command == command)
        .map(|source| source.connector.as_ref())
}

async fn recorded_method_count(methods: &Arc<Mutex<Vec<String>>>, method: &str) -> usize {
    methods
        .lock()
        .await
        .iter()
        .filter(|recorded| recorded.as_str() == method)
        .count()
}

#[tokio::test]
async fn collect_snapshot_with_connectors_uses_fallback_connector_for_missing_optional_metric(
) -> Result<()> {
    let (primary_rpc_url, primary_server_task) =
        start_mock_rpc_server_with_profile(MockRpcProfile::NoLeaderSchedule).await?;
    let (fallback_rpc_url, fallback_server_task) = start_mock_rpc_server().await?;
    let config = Config {
        network: "devnet".to_string(),
        connectors: vec![
            ConnectorConfig {
                name: "local-validator".to_string(),
                provider: "solana-validator".to_string(),
                features: Vec::new(),
                rpc: Some(crate::config::RpcTransportConfig::new(primary_rpc_url)),
                ..ConnectorConfig::default()
            },
            ConnectorConfig {
                name: "helius-fallback".to_string(),
                provider: "helius".to_string(),
                features: Vec::new(),
                rpc: Some(crate::config::RpcTransportConfig::new(fallback_rpc_url)),
                ..ConnectorConfig::default()
            },
        ],
        modes: modes_for(&["local-validator", "helius-fallback"], false),
        watched_validators: vec![WatchedValidatorConfig {
            identity: Some("id-1".to_string()),
            vote_account: Some("vote-1".to_string()),
            labels: vec!["operator:test".to_string()],
        }],
        ..Config::default()
    };
    let sources = SourceChain::from_config(&config, crate::RuntimeTelemetry::unobserved())?;
    let snapshot = collect_snapshot(&sources, &config).await?;

    primary_server_task.abort();
    fallback_server_task.abort();

    let epoch_info_source = snapshot
        .data_sources
        .iter()
        .find(|source| source.command == CommandId::EpochInfo)
        .ok_or_else(|| anyhow!("missing epoch-info source"))?;
    let current_schedule_source = snapshot
        .data_sources
        .iter()
        .find(|source| source.command == CommandId::CurrentLeaderSchedule)
        .ok_or_else(|| anyhow!("missing current-leader-schedule source"))?;
    let next_schedule_source = snapshot
        .data_sources
        .iter()
        .find(|source| source.command == CommandId::NextLeaderSchedule)
        .ok_or_else(|| anyhow!("missing next-leader-schedule source"))?;

    assert_eq!(epoch_info_source.connector.as_ref(), "local-validator");
    assert_eq!(epoch_info_source.provider.as_ref(), "solana-validator");
    assert_eq!(
        current_schedule_source.connector.as_ref(),
        "helius-fallback"
    );
    assert_eq!(current_schedule_source.provider.as_ref(), "helius");
    assert_eq!(current_schedule_source.precedence, 2);
    assert_eq!(next_schedule_source.connector.as_ref(), "helius-fallback");
    assert_eq!(snapshot.watched_validators[0].future_leader_slots, 3);

    Ok(())
}

#[tokio::test]
async fn collect_snapshot_with_connectors_uses_fallback_connector_for_missing_required_metric(
) -> Result<()> {
    let (primary_rpc_url, primary_server_task) =
        start_mock_rpc_server_with_profile(MockRpcProfile::NoEpochInfo).await?;
    let (fallback_rpc_url, fallback_server_task) = start_mock_rpc_server().await?;
    let config = Config {
        network: "devnet".to_string(),
        connectors: vec![
            ConnectorConfig {
                name: "local-validator".to_string(),
                provider: "solana-validator".to_string(),
                features: Vec::new(),
                rpc: Some(crate::config::RpcTransportConfig::new(primary_rpc_url)),
                ..ConnectorConfig::default()
            },
            ConnectorConfig {
                name: "helius-fallback".to_string(),
                provider: "helius".to_string(),
                features: Vec::new(),
                rpc: Some(crate::config::RpcTransportConfig::new(fallback_rpc_url)),
                ..ConnectorConfig::default()
            },
        ],
        modes: modes_for(&["local-validator", "helius-fallback"], false),
        ..Config::default()
    };
    let sources = SourceChain::from_config(&config, crate::RuntimeTelemetry::unobserved())?;
    let snapshot = collect_snapshot(&sources, &config).await?;

    primary_server_task.abort();
    fallback_server_task.abort();

    let epoch_info_source = snapshot
        .data_sources
        .iter()
        .find(|source| source.command == CommandId::EpochInfo)
        .ok_or_else(|| anyhow!("missing epoch-info source"))?;
    let vote_accounts_source = snapshot
        .data_sources
        .iter()
        .find(|source| source.command == CommandId::VoteAccounts)
        .ok_or_else(|| anyhow!("missing vote-accounts source"))?;

    assert_eq!(epoch_info_source.connector.as_ref(), "helius-fallback");
    assert_eq!(epoch_info_source.provider.as_ref(), "helius");
    assert_eq!(epoch_info_source.precedence, 2);
    assert_eq!(vote_accounts_source.connector.as_ref(), "local-validator");
    assert_eq!(vote_accounts_source.provider.as_ref(), "solana-validator");

    Ok(())
}

#[tokio::test]
async fn collect_snapshot_resolves_each_command_from_its_own_route() -> Result<()> {
    let (first_rpc_url, first_server_task) = start_mock_rpc_server().await?;
    let (second_rpc_url, second_server_task) = start_mock_rpc_server().await?;
    let mut modes = modes_for(&["first", "second"], false);
    modes.consensus.commands.vote_accounts = vec!["second".to_string(), "first".to_string()];
    let config = Config {
        network: "devnet".to_string(),
        connectors: vec![
            ConnectorConfig {
                name: "first".to_string(),
                provider: "solana-validator".to_string(),
                features: Vec::new(),
                rpc: Some(crate::config::RpcTransportConfig::new(first_rpc_url)),
                ..ConnectorConfig::default()
            },
            ConnectorConfig {
                name: "second".to_string(),
                provider: "helius".to_string(),
                features: Vec::new(),
                rpc: Some(crate::config::RpcTransportConfig::new(second_rpc_url)),
                ..ConnectorConfig::default()
            },
        ],
        modes,
        ..Config::default()
    };
    let sources = SourceChain::from_config(&config, crate::RuntimeTelemetry::unobserved())?;
    let snapshot = collect_snapshot(&sources, &config).await?;

    first_server_task.abort();
    second_server_task.abort();

    let epoch_info_source = snapshot
        .data_sources
        .iter()
        .find(|source| source.command == CommandId::EpochInfo)
        .ok_or_else(|| anyhow!("missing epoch-info source"))?;
    let vote_accounts_source = snapshot
        .data_sources
        .iter()
        .find(|source| source.command == CommandId::VoteAccounts)
        .ok_or_else(|| anyhow!("missing vote-accounts source"))?;

    assert_eq!(epoch_info_source.connector.as_ref(), "first");
    assert_eq!(epoch_info_source.precedence, 1);
    assert_eq!(vote_accounts_source.connector.as_ref(), "second");
    assert_eq!(vote_accounts_source.precedence, 1);
    Ok(())
}

#[tokio::test]
async fn required_command_preserves_every_failed_route_connector() -> Result<()> {
    let (first_rpc_url, first_server_task) =
        start_mock_rpc_server_with_profile(MockRpcProfile::NoEpochInfo).await?;
    let (second_rpc_url, second_server_task) =
        start_mock_rpc_server_with_profile(MockRpcProfile::NoEpochInfo).await?;
    let config = Config {
        network: "devnet".to_string(),
        connectors: vec![
            ConnectorConfig {
                name: "first".to_string(),
                provider: "solana-validator".to_string(),
                features: Vec::new(),
                rpc: Some(crate::config::RpcTransportConfig::new(first_rpc_url)),
                ..ConnectorConfig::default()
            },
            ConnectorConfig {
                name: "second".to_string(),
                provider: "helius".to_string(),
                features: Vec::new(),
                rpc: Some(crate::config::RpcTransportConfig::new(second_rpc_url)),
                ..ConnectorConfig::default()
            },
        ],
        modes: modes_for(&["first", "second"], false),
        ..Config::default()
    };
    let sources = SourceChain::from_config(&config, crate::RuntimeTelemetry::unobserved())?;

    let Err(error) = collect_snapshot(&sources, &config).await else {
        anyhow::bail!("all failed required command routes must fail collection");
    };
    first_server_task.abort();
    second_server_task.abort();

    let message = format!("{error:#}");
    assert!(message.contains("connector route exhausted for required command epoch-info"));
    assert!(message.contains("connector \"first\" (provider \"solana-validator\")"));
    assert!(message.contains("connector \"second\" (provider \"helius\")"));
    Ok(())
}

#[tokio::test]
async fn reward_history_propagates_minimum_ledger_slot_failure() -> Result<()> {
    let (rpc_url, server_task) =
        start_mock_rpc_server_with_profile(MockRpcProfile::NoMinimumLedgerSlot).await?;
    let config = Config {
        network: "devnet".to_string(),
        connectors: vec![ConnectorConfig {
            name: "historical-rpc".to_string(),
            provider: "mock-solana".to_string(),
            features: vec![ProviderFeature::HistoricalData],
            rpc: Some(crate::config::RpcTransportConfig::new(rpc_url)),
            ..ConnectorConfig::default()
        }],
        modes: modes_for(&["historical-rpc"], true),
        ..Config::default()
    };
    let sources = SourceChain::from_config(&config, crate::RuntimeTelemetry::unobserved())?;

    let result = collect_snapshot(&sources, &config).await;
    server_task.abort();
    let Err(error) = result else {
        anyhow::bail!("minimumLedgerSlot failure must fail required reward-history collection");
    };

    let message = format!("{error:#}");
    assert!(message
        .contains("connector route exhausted for required command reward-epoch-block-production"));
    assert!(message.contains("minimumLedgerSlot failed with JSON-RPC code -32601"));
    Ok(())
}

#[tokio::test]
async fn economics_succeeds_when_optional_leadership_command_is_unavailable() -> Result<()> {
    let (rpc_url, server_task) =
        start_mock_rpc_server_with_profile(MockRpcProfile::NoCurrentBlockProduction).await?;
    let config = Config {
        network: "devnet".to_string(),
        connectors: vec![ConnectorConfig {
            name: "primary".to_string(),
            provider: "mock-solana".to_string(),
            features: vec![ProviderFeature::HistoricalData],
            rpc: Some(crate::config::RpcTransportConfig::new(rpc_url)),
            ..ConnectorConfig::default()
        }],
        modes: modes_for(&["primary"], true),
        watched_validators: vec![WatchedValidatorConfig {
            identity: Some("id-1".to_string()),
            vote_account: Some("vote-1".to_string()),
            labels: vec!["operator:test".to_string()],
        }],
        ..Config::default()
    };
    let sources = SourceChain::from_config(&config, crate::RuntimeTelemetry::unobserved())?;

    ensure_economics_supported(&sources, &config).await?;
    let snapshot = collect_snapshot(&sources, &config).await?;
    server_task.abort();

    assert!(snapshot
        .mode_statuses
        .iter()
        .any(|status| status.mode == ModeId::Leadership && !status.up));
    assert!(snapshot
        .mode_statuses
        .iter()
        .any(|status| status.mode == ModeId::Economics && status.up));
    assert!(!snapshot
        .data_sources
        .iter()
        .any(|source| source.command == CommandId::CurrentBlockProduction));
    assert!(snapshot
        .data_sources
        .iter()
        .any(|source| source.command == CommandId::RewardEpochBlockProduction));
    Ok(())
}

#[tokio::test]
async fn block_fee_reward_identity_mismatch_falls_back_to_the_next_connector() -> Result<()> {
    let (wrong_url, wrong_task) =
        start_mock_rpc_server_with_profile(MockRpcProfile::WrongBlockFeeRewardIdentity).await?;
    let (fallback_url, fallback_task) = start_mock_rpc_server().await?;
    let features = vec![
        ProviderFeature::HistoricalData,
        ProviderFeature::BlockFeeRewards,
    ];
    let mut config = Config {
        network: "devnet".to_string(),
        connectors: vec![
            ConnectorConfig {
                name: "wrong-reward".to_string(),
                provider: "mock-primary".to_string(),
                features: features.clone(),
                rpc: Some(crate::config::RpcTransportConfig::new(wrong_url)),
                ..ConnectorConfig::default()
            },
            ConnectorConfig {
                name: "fallback-reward".to_string(),
                provider: "mock-fallback".to_string(),
                features,
                rpc: Some(crate::config::RpcTransportConfig::new(fallback_url)),
                ..ConnectorConfig::default()
            },
        ],
        modes: modes_for(&["wrong-reward", "fallback-reward"], true),
        ..Config::default()
    };
    config.modes.economics.commands.recent_block_fee_rewards =
        vec!["wrong-reward".to_string(), "fallback-reward".to_string()];
    let sources = SourceChain::from_config(&config, crate::RuntimeTelemetry::unobserved())?;

    let snapshot = collect_snapshot(&sources, &config).await?;
    wrong_task.abort();
    fallback_task.abort();

    assert_eq!(
        snapshot
            .economics
            .as_ref()
            .and_then(|economics| economics.recent_block_fee_rewards),
        Some(RecentBlockFeeRewards {
            latest_slot: 805,
            latest_reward_lamports: 3_000_000,
            total_reward_lamports: 4_000_000,
            sampled_block_count: 2,
        })
    );
    assert!(snapshot.data_sources.iter().any(|source| {
        source.command == CommandId::RecentBlockFeeRewards
            && source.connector.as_ref() == "fallback-reward"
    }));
    Ok(())
}
