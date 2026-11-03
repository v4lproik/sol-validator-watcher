use super::*;
#[tokio::test]
async fn get_block_production_for_range_batches_large_requests() -> Result<()> {
    let (rpc_url, server_task) = start_mock_rpc_server().await?;
    let client = RpcClient::new(
        rpc_url,
        Duration::from_secs(5),
        "confirmed".to_string(),
        crate::watcher_state::RpcTelemetry::unobserved(),
    )?;

    let block_production = client.get_block_production_for_range(700, 10_799).await?;

    server_task.abort();

    assert_eq!(block_production.by_identity.get("id-1"), Some(&(7, 6)));

    Ok(())
}

#[tokio::test]
async fn call_optional_surfaces_json_rpc_errors_from_http_400() -> Result<()> {
    let (rpc_url, server_task) = start_mock_rpc_server().await?;
    let mailbox = WatcherStateMailbox::new();
    let telemetry = mailbox.rpc_telemetry();
    let metrics = Arc::new(Metrics::new()?);
    let (state, actor) = mailbox.start("rpc-telemetry-test", metrics);
    let client = RpcClient::new(
        rpc_url,
        Duration::from_secs(5),
        "confirmed".to_string(),
        telemetry,
    )?;
    let mut tasks = TaskManager::new();
    tasks.spawn("watcher-state-actor", actor.run(tasks.shutdown_handle()));
    tasks.spawn("rpc-client-scenario", async move {
        state.begin_poll().await?;
        let err = match client
            .call_optional::<RpcValueResponse<BlockProduction>, _>(
                "getBlockProduction",
                [GetBlockProductionConfig {
                    commitment: "confirmed",
                    range: Some(SlotRange {
                        first_slot: 1,
                        last_slot: 5_001,
                    }),
                }],
            )
            .await
        {
            Ok(_) => panic!("expected oversized block production request to fail"),
            Err(err) => err,
        };
        let completion = state.complete_poll().await?;
        server_task.abort();

        assert!(err
            .to_string()
            .contains("getBlockProduction failed with JSON-RPC code -32600"));
        assert_eq!(completion.state.rpc_responses.client_error_4xx, 1);
        assert_eq!(completion.state.rpc_responses.failed_attempts, 1);
        assert!(!completion.state.ready);
        Ok(TaskExit::ShutdownRequested)
    });

    tasks.run().await
}

#[tokio::test]
async fn get_inflation_rewards_batches_requests_at_rpc_limit() -> Result<()> {
    let (rpc_url, server_task) = start_mock_rpc_server().await?;
    let client = RpcClient::new(
        rpc_url,
        Duration::from_secs(5),
        "confirmed".to_string(),
        crate::watcher_state::RpcTelemetry::unobserved(),
    )?;
    let addresses = (0..205)
        .map(|index| format!("vote-{index}"))
        .collect::<Vec<_>>();

    let rewards = client.get_inflation_rewards(&addresses, 8).await?;

    server_task.abort();

    assert_eq!(rewards.len(), 205);
    assert_eq!(
        rewards.get("vote-0"),
        Some(&RewardSample {
            epoch: 8,
            amount_lamports: 12_000_000_000,
        })
    );
    assert_eq!(
        rewards.get("vote-204"),
        Some(&RewardSample {
            epoch: 8,
            amount_lamports: 12_000_000_000,
        })
    );

    Ok(())
}

#[tokio::test]
async fn missing_vote_account_space_preserves_balances_and_omits_vat() -> Result<()> {
    let (rpc_url, server_task) =
        start_mock_rpc_server_with_profile(MockRpcProfile::NoAccountSpace).await?;
    let client = RpcClient::new(
        rpc_url,
        Duration::from_secs(5),
        "confirmed".to_string(),
        crate::watcher_state::RpcTelemetry::unobserved(),
    )?;

    let balances = client.get_account_balances("id-1", "vote-1").await?;
    server_task.abort();
    assert_eq!(balances.identity_lamports, 3_000_000_000);
    assert_eq!(balances.vote_account_lamports, 2_500_000_000);
    assert_eq!(balances.vote_account_rent_exempt_minimum_lamports, None);

    let metrics = build_economics_metrics(
        &EconomicsTarget {
            identity: "id-1".to_string(),
            vote_account: "vote-1".to_string(),
        },
        balances,
    );
    assert_eq!(metrics.vat_reserve_lamports, None);
    assert_eq!(metrics.vat_epochs_available, None);
    assert_eq!(metrics.vat_next_epoch_funded, None);
    Ok(())
}
#[tokio::test]
async fn rpc_health_errors_do_not_expose_secret_query_parameters() -> Result<()> {
    let (rpc_url, server_task) =
        start_mock_rpc_server_with_profile(MockRpcProfile::Unhealthy).await?;
    let secret = "do-not-expose-this-api-key";
    let client = RpcClient::new(
        format!("{rpc_url}/?api-key={secret}"),
        Duration::from_secs(5),
        "confirmed".to_string(),
        crate::watcher_state::RpcTelemetry::unobserved(),
    )?;

    let Err(error) = client.ensure_healthy().await else {
        anyhow::bail!("unhealthy RPC should fail");
    };
    server_task.abort();
    let message = format!("{error:#}");

    assert!(message.contains("unexpected getHealth result"));
    assert!(
        !message.contains(secret),
        "RPC health errors must redact secret URL query parameters"
    );
    Ok(())
}

#[tokio::test]
async fn connector_capability_probe_rejects_extended_profile_without_exposing_endpoint_secrets(
) -> Result<()> {
    let (rpc_url, server_task) =
        start_mock_rpc_server_with_profile(MockRpcProfile::NoPerformanceSamples).await?;
    let secret = "do-not-expose-capability-probe-key";
    let mut config = Config {
        connectors: vec![ConnectorConfig {
            name: "limited-rpc".to_string(),
            provider: "mock-provider".to_string(),
            features: Vec::new(),
            solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
            rpc_url: Some(format!("{rpc_url}/?api-key={secret}")),
            ..ConnectorConfig::default()
        }],
        ..Config::default()
    };
    let sources =
        SourceChain::from_config(&config, crate::watcher_state::RpcTelemetry::unobserved())?;
    let Err(error) = sources.verify_capabilities().await else {
        anyhow::bail!("extended compatibility without extended methods should fail");
    };
    let message = format!("{error:#}");
    assert!(message.contains("connector \"limited-rpc\""), "{message}");
    assert!(message.contains("provider \"mock-provider\""), "{message}");
    assert!(message.contains("RPC endpoint"), "{message}");
    assert!(
        message.contains("solana_rpc_compatibility = \"extended\""),
        "{message}"
    );
    assert!(message.contains("getRecentPerformanceSamples"), "{message}");
    assert!(!message.contains(secret), "{message}");

    config.connectors[0].solana_rpc_compatibility = Some(SolanaRpcCompatibility::Core);
    SourceChain::from_config(&config, crate::watcher_state::RpcTelemetry::unobserved())?
        .verify_capabilities()
        .await?;
    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn connector_capability_probe_uses_only_declared_core_methods() -> Result<()> {
    let (rpc_url, server_task) =
        start_mock_rpc_server_with_profile(MockRpcProfile::NoSlotLeader).await?;
    let config = Config {
        connectors: vec![ConnectorConfig {
            name: "core-rpc".to_string(),
            provider: "mock-provider".to_string(),
            features: Vec::new(),
            solana_rpc_compatibility: Some(SolanaRpcCompatibility::Core),
            rpc_url: Some(rpc_url),
            ..ConnectorConfig::default()
        }],
        ..Config::default()
    };

    SourceChain::from_config(&config, crate::watcher_state::RpcTelemetry::unobserved())?
        .verify_capabilities()
        .await?;
    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn connector_capability_probe_calls_exact_core_and_extended_method_sets() -> Result<()> {
    let (core_url, core_task, core_methods) =
        start_recording_mock_rpc_server(MockRpcProfile::StrictCapabilityParams).await?;
    let core_config = Config {
        connectors: vec![ConnectorConfig {
            name: "core-rpc".to_string(),
            provider: "mock-provider".to_string(),
            features: Vec::new(),
            solana_rpc_compatibility: Some(SolanaRpcCompatibility::Core),
            rpc_url: Some(core_url),
            ..ConnectorConfig::default()
        }],
        ..Config::default()
    };
    SourceChain::from_config(
        &core_config,
        crate::watcher_state::RpcTelemetry::unobserved(),
    )?
    .verify_capabilities()
    .await?;
    let mut core_methods = core_methods.lock().await.clone();
    core_methods.sort();
    assert_eq!(
        core_methods.iter().map(String::as_str).collect::<Vec<_>>(),
        [
            "getEpochInfo",
            "getHealth",
            "getLeaderSchedule",
            "getVoteAccounts"
        ]
    );
    core_task.abort();

    let (extended_url, extended_task, extended_methods) =
        start_recording_mock_rpc_server(MockRpcProfile::StrictCapabilityParams).await?;
    let extended_config = Config {
        connectors: vec![ConnectorConfig {
            name: "extended-rpc".to_string(),
            provider: "mock-provider".to_string(),
            features: Vec::new(),
            solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
            rpc_url: Some(extended_url),
            ..ConnectorConfig::default()
        }],
        ..Config::default()
    };
    SourceChain::from_config(
        &extended_config,
        crate::watcher_state::RpcTelemetry::unobserved(),
    )?
    .verify_capabilities()
    .await?;
    let mut extended_methods = extended_methods.lock().await.clone();
    extended_methods.sort();
    assert_eq!(
        extended_methods
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "getBlockProduction",
            "getEpochInfo",
            "getHealth",
            "getInflationReward",
            "getLeaderSchedule",
            "getMinimumBalanceForRentExemption",
            "getMultipleAccounts",
            "getRecentPerformanceSamples",
            "getVoteAccounts",
            "minimumLedgerSlot"
        ]
    );
    extended_task.abort();
    Ok(())
}

#[tokio::test]
async fn connector_capability_probe_validates_method_specific_payloads() -> Result<()> {
    let (rpc_url, server_task) =
        start_mock_rpc_server_with_profile(MockRpcProfile::StrictCapabilityParams).await?;
    let config = Config {
        connectors: vec![ConnectorConfig {
            name: "strict-extended-rpc".to_string(),
            provider: "mock-provider".to_string(),
            features: vec![ProviderFeature::HistoricalData],
            solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
            rpc_url: Some(rpc_url),
            ..ConnectorConfig::default()
        }],
        ..Config::default()
    };

    SourceChain::from_config(&config, crate::watcher_state::RpcTelemetry::unobserved())?
        .verify_capabilities()
        .await?;
    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn connector_capability_probe_checks_every_extended_rpc_method() -> Result<()> {
    for (profile, expected_method) in [
        (MockRpcProfile::NoInflationRewards, "getInflationReward"),
        (MockRpcProfile::NoMinimumLedgerSlot, "minimumLedgerSlot"),
    ] {
        let (rpc_url, server_task) = start_mock_rpc_server_with_profile(profile).await?;
        let config = Config {
            connectors: vec![ConnectorConfig {
                name: "extended-rpc".to_string(),
                provider: "mock-provider".to_string(),
                features: Vec::new(),
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
                rpc_url: Some(rpc_url),
                ..ConnectorConfig::default()
            }],
            ..Config::default()
        };

        let Err(error) =
            SourceChain::from_config(&config, crate::watcher_state::RpcTelemetry::unobserved())?
                .verify_capabilities()
                .await
        else {
            anyhow::bail!(
                "{expected_method} must be required by extended Solana RPC compatibility"
            );
        };
        let message = format!("{error:#}");
        assert!(
            message.contains("solana_rpc_compatibility = \"extended\""),
            "{message}"
        );
        assert!(message.contains(expected_method), "{message}");
        assert!(
            !message.contains("provider feature \"historical-data\""),
            "{message}"
        );
        server_task.abort();
    }
    Ok(())
}

#[tokio::test]
async fn connector_capability_probe_rejects_empty_required_results() -> Result<()> {
    for profile in [
        MockRpcProfile::NullPerformanceSamples,
        MockRpcProfile::MissingPerformanceSamplesResult,
    ] {
        let (rpc_url, server_task) = start_mock_rpc_server_with_profile(profile).await?;
        let config = Config {
            connectors: vec![ConnectorConfig {
                name: "extended-rpc".to_string(),
                provider: "mock-provider".to_string(),
                features: Vec::new(),
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
                rpc_url: Some(rpc_url),
                ..ConnectorConfig::default()
            }],
            ..Config::default()
        };

        let Err(error) =
            SourceChain::from_config(&config, crate::watcher_state::RpcTelemetry::unobserved())?
                .verify_capabilities()
                .await
        else {
            anyhow::bail!("empty required RPC results must fail capability validation");
        };
        let message = format!("{error:#}");
        assert!(message.contains("getRecentPerformanceSamples"), "{message}");
        assert!(message.contains("returned no result"), "{message}");
        server_task.abort();
    }
    Ok(())
}

#[tokio::test]
async fn connector_capability_probe_reports_declared_historical_data_feature() -> Result<()> {
    let (rpc_url, server_task) =
        start_mock_rpc_server_with_profile(MockRpcProfile::NoInflationRewards).await?;
    let config = Config {
        connectors: vec![ConnectorConfig {
            name: "archive-rpc".to_string(),
            provider: "mock-provider".to_string(),
            features: vec![ProviderFeature::HistoricalData],
            solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
            rpc_url: Some(rpc_url),
            ..ConnectorConfig::default()
        }],
        ..Config::default()
    };
    let Err(error) =
        SourceChain::from_config(&config, crate::watcher_state::RpcTelemetry::unobserved())?
            .verify_capabilities()
            .await
    else {
        anyhow::bail!("missing historical-data RPC method should fail");
    };
    let message = format!("{error:#}");
    assert!(
        message.contains("provider feature \"historical-data\""),
        "{message}"
    );
    assert!(message.contains("getInflationReward"), "{message}");
    server_task.abort();
    Ok(())
}
