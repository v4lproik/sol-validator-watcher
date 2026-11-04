use super::*;
#[tokio::test]
async fn rpc_client_collect_snapshot_and_validate_economics_against_mock_rpc() -> Result<()> {
    let (rpc_url, server_task) = start_mock_rpc_server().await?;
    let mut config = Config {
        network: "devnet".to_string(),
        connectors: vec![ConnectorConfig {
            name: "primary".to_string(),
            provider: "mock-solana".to_string(),
            features: vec![ProviderFeature::HistoricalData],
            rpc_url: Some(rpc_url.clone()),
            rpc_timeout_sec: 5,
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
    let client = RpcClient::new(
        rpc_url,
        Duration::from_secs(5),
        config.connectors[0].commitment_config()?,
        crate::watcher_state::RpcTelemetry::unobserved().for_connector("mock-rpc"),
    )?;

    assert!(client.node_health().await?.is_healthy());
    let sources =
        SourceChain::from_config(&config, crate::watcher_state::RpcTelemetry::unobserved())?;
    ensure_economics_supported(&sources, &config).await?;
    let snapshot = collect_snapshot(&sources, &config).await?;

    server_task.abort();

    assert_eq!(snapshot.network, "devnet");
    assert_eq!(snapshot.epoch, 9);
    assert_eq!(snapshot.slot, 810);
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
    assert_eq!(snapshot.vote_credits_sample_epoch, Some(8));
    assert_eq!(snapshot.reward_sample_epoch, Some(8));
    assert_eq!(snapshot.watched_validators.len(), 1);
    assert_eq!(
        snapshot.watched_validators[0].latest_epoch_reward_lamports,
        12_000_000_000
    );
    assert_eq!(
        snapshot.watched_validators[0].latest_epoch_rewarded_blocks_produced,
        3
    );
    assert_eq!(snapshot.watched_validators[0].blocks_produced, 4);
    assert_eq!(snapshot.watched_validators[0].future_leader_slots, 3);

    assert_eq!(snapshot.epoch_progress_pct, 10.0);
    assert_eq!(snapshot.epoch_slots_remaining, 90);
    assert_eq!(snapshot.average_slot_time_seconds, Some(0.4));
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
    let economics = snapshot
        .economics
        .as_ref()
        .ok_or_else(|| anyhow!("missing economics metrics"))?;
    assert_eq!(economics.identity_balance_lamports, 3_000_000_000);
    assert_eq!(economics.vote_account_balance_lamports, 2_500_000_000);
    assert_eq!(
        economics.vote_account_rent_exempt_minimum_lamports,
        Some(900_000_000)
    );
    assert_eq!(economics.vat_reserve_lamports, Some(1_600_000_000));
    assert_eq!(economics.vat_epochs_available, Some(2));
    assert_eq!(economics.vat_next_epoch_funded, Some(true));
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
            rpc_url: Some(rpc_url),
            rpc_timeout_sec: 5,
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
    let sources =
        SourceChain::from_config(&config, crate::watcher_state::RpcTelemetry::unobserved())?;
    let snapshot = collect_snapshot(&sources, &config).await?;
    server_task.abort();

    assert_eq!(snapshot.average_slot_time_seconds, None);
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
                rpc_url: Some(primary_rpc_url),
                rpc_timeout_sec: 5,
                ..ConnectorConfig::default()
            },
            ConnectorConfig {
                name: "helius-fallback".to_string(),
                provider: "helius".to_string(),
                features: Vec::new(),
                rpc_url: Some(fallback_rpc_url),
                rpc_timeout_sec: 5,
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
    let sources =
        SourceChain::from_config(&config, crate::watcher_state::RpcTelemetry::unobserved())?;
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
                rpc_url: Some(primary_rpc_url),
                rpc_timeout_sec: 5,
                ..ConnectorConfig::default()
            },
            ConnectorConfig {
                name: "helius-fallback".to_string(),
                provider: "helius".to_string(),
                features: Vec::new(),
                rpc_url: Some(fallback_rpc_url),
                rpc_timeout_sec: 5,
                ..ConnectorConfig::default()
            },
        ],
        modes: modes_for(&["local-validator", "helius-fallback"], false),
        ..Config::default()
    };
    let sources =
        SourceChain::from_config(&config, crate::watcher_state::RpcTelemetry::unobserved())?;
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
                rpc_url: Some(first_rpc_url),
                rpc_timeout_sec: 5,
                ..ConnectorConfig::default()
            },
            ConnectorConfig {
                name: "second".to_string(),
                provider: "helius".to_string(),
                features: Vec::new(),
                rpc_url: Some(second_rpc_url),
                rpc_timeout_sec: 5,
                ..ConnectorConfig::default()
            },
        ],
        modes,
        ..Config::default()
    };
    let sources =
        SourceChain::from_config(&config, crate::watcher_state::RpcTelemetry::unobserved())?;
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
                rpc_url: Some(first_rpc_url),
                rpc_timeout_sec: 5,
                ..ConnectorConfig::default()
            },
            ConnectorConfig {
                name: "second".to_string(),
                provider: "helius".to_string(),
                features: Vec::new(),
                rpc_url: Some(second_rpc_url),
                rpc_timeout_sec: 5,
                ..ConnectorConfig::default()
            },
        ],
        modes: modes_for(&["first", "second"], false),
        ..Config::default()
    };
    let sources =
        SourceChain::from_config(&config, crate::watcher_state::RpcTelemetry::unobserved())?;

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
            rpc_url: Some(rpc_url),
            rpc_timeout_sec: 5,
            ..ConnectorConfig::default()
        }],
        modes: modes_for(&["historical-rpc"], true),
        ..Config::default()
    };
    let sources =
        SourceChain::from_config(&config, crate::watcher_state::RpcTelemetry::unobserved())?;

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
            rpc_url: Some(rpc_url),
            rpc_timeout_sec: 5,
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
    let sources =
        SourceChain::from_config(&config, crate::watcher_state::RpcTelemetry::unobserved())?;

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
