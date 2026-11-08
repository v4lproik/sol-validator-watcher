use super::*;

#[test]
fn rpc_client_constructor_rejects_bad_urls_without_exposing_queries() -> Result<()> {
    let secret = "rpc-constructor-secret";
    for (url, expected) in [
        (
            format!("ftp://rpc.example/?api-key={secret}"),
            "Solana RPC URL must use http:// or https:// with a host",
        ),
        (
            format!("http:///rpc?api-key={secret}"),
            "Solana RPC URL must use http:// or https:// with a host",
        ),
    ] {
        let Err(error) = RpcClient::try_new(
            &rpc_transport(&url, 1),
            crate::watcher_state::RpcTelemetry::unobserved().for_connector("mock-rpc"),
        ) else {
            anyhow::bail!("invalid RPC URL should fail during client construction");
        };
        let message = format!("{error:#}");
        assert!(message.contains(expected), "{message}");
        assert!(
            !message.contains(secret),
            "RPC constructor errors must redact secret URL query parameters"
        );
    }
    Ok(())
}

#[tokio::test]
async fn get_block_production_for_range_batches_large_requests() -> Result<()> {
    let (rpc_url, server_task) = start_mock_rpc_server().await?;
    let client = RpcClient::try_new(
        &rpc_transport(&rpc_url, 5),
        crate::watcher_state::RpcTelemetry::unobserved().for_connector("mock-rpc"),
    )?;

    let block_production = client.get_block_production_for_range(700, 10_799).await?;

    server_task.abort();

    assert_eq!(block_production.by_identity.get("id-1"), Some(&(7, 6)));

    Ok(())
}

#[tokio::test]
async fn get_cluster_nodes_decodes_validator_protocol_metadata() -> Result<()> {
    let (rpc_url, server_task) = start_mock_rpc_server().await?;
    let client = RpcClient::try_new(
        &rpc_transport(&rpc_url, 5),
        crate::watcher_state::RpcTelemetry::unobserved().for_connector("mock-rpc"),
    )?;

    let nodes = client.get_cluster_nodes().await?;
    server_task.abort();

    let node = nodes
        .first()
        .ok_or_else(|| anyhow!("missing cluster node"))?;
    assert_eq!(node.pubkey, "id-1");
    assert_eq!(node.version.as_deref(), Some("3.1.8"));
    assert_eq!(node.feature_set, Some(2_891_131_721));
    assert_eq!(node.shred_version, Some(56_123));
    Ok(())
}

#[tokio::test]
async fn finalized_block_fee_rewards_cache_produced_blocks_and_retry_skipped_slots() -> Result<()> {
    let (rpc_url, server_task, requests) =
        start_request_recording_mock_rpc_server(MockRpcProfile::Default).await?;
    let mut transport = rpc_transport(&rpc_url, 5);
    transport.commitment = "finalized".to_string();
    let client = RpcClient::try_new(
        &transport,
        crate::watcher_state::RpcTelemetry::unobserved().for_connector("block-reward-rpc"),
    )?;

    let rewards = client
        .get_recent_block_fee_rewards(&[805, 802, 800], "id-1")
        .await?
        .ok_or_else(|| anyhow!("missing recent block fee rewards"))?;
    let repeated_rewards = client
        .get_recent_block_fee_rewards(&[805, 802, 800], "id-1")
        .await?
        .ok_or_else(|| anyhow!("missing cached recent block fee rewards"))?;
    server_task.abort();
    assert_eq!(
        rewards,
        RecentBlockFeeRewards {
            latest_slot: 805,
            latest_reward_lamports: 3_000_000,
            total_reward_lamports: 4_000_000,
            sampled_block_count: 2,
        }
    );
    assert_eq!(repeated_rewards, rewards);
    let recorded = requests.lock().await;
    let batch = recorded
        .first()
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("getBlock requests were not sent as one JSON-RPC batch"))?;
    assert_eq!(batch.len(), 3);
    assert_eq!(recorded.len(), 2);
    let retry_batch = recorded[1]
        .as_array()
        .ok_or_else(|| anyhow!("uncached skipped slot was not retried as a JSON-RPC batch"))?;
    assert_eq!(
        retry_batch,
        &[json!({
            "jsonrpc": "2.0",
            "id": 802,
            "method": "getBlock",
            "params": [
                802,
                {
                    "commitment": "finalized",
                    "transactionDetails": "none",
                    "rewards": true
                }
            ]
        })]
    );
    assert_eq!(
        batch[0],
        json!({
            "jsonrpc": "2.0",
            "id": 805,
            "method": "getBlock",
            "params": [
                805,
                {
                    "commitment": "finalized",
                    "transactionDetails": "none",
                    "rewards": true
                }
            ]
        })
    );
    Ok(())
}

#[tokio::test]
async fn confirmed_block_fee_rewards_are_refetched() -> Result<()> {
    let (rpc_url, server_task, requests) =
        start_request_recording_mock_rpc_server(MockRpcProfile::Default).await?;
    let client = RpcClient::try_new(
        &rpc_transport(&rpc_url, 5),
        crate::watcher_state::RpcTelemetry::unobserved().for_connector("block-reward-rpc"),
    )?;

    for _ in 0..2 {
        client
            .get_recent_block_fee_rewards(&[805, 802, 800], "id-1")
            .await?
            .ok_or_else(|| anyhow!("missing confirmed recent block fee rewards"))?;
    }
    server_task.abort();

    let recorded = requests.lock().await;
    assert_eq!(recorded.len(), 2);
    for request in recorded.iter() {
        let batch = request
            .as_array()
            .ok_or_else(|| anyhow!("getBlock requests were not sent as a JSON-RPC batch"))?;
        assert_eq!(batch.len(), 3);
        assert!(batch.iter().all(|request| {
            request
                .pointer("/params/1/commitment")
                .and_then(Value::as_str)
                == Some("confirmed")
        }));
    }
    Ok(())
}
#[tokio::test]
async fn missing_transaction_breakdown_preserves_total_performance_rates() -> Result<()> {
    let (rpc_url, server_task) =
        start_mock_rpc_server_with_profile(MockRpcProfile::MissingPerformanceTransactionSplit)
            .await?;
    let client = RpcClient::try_new(
        &rpc_transport(&rpc_url, 5),
        crate::watcher_state::RpcTelemetry::unobserved().for_connector("mock-rpc"),
    )?;

    let performance = client
        .get_cluster_performance_metrics()
        .await?
        .ok_or_else(|| anyhow!("missing performance sample"))?;
    server_task.abort();

    assert_eq!(performance.average_slot_time_seconds, Some(0.4));
    assert_eq!(performance.transactions_per_second, 25.0);
    assert_eq!(performance.vote_transactions_per_second, None);
    assert_eq!(performance.non_vote_transactions_per_second, None);
    assert_eq!(performance.slots_per_second, 2.5);
    Ok(())
}

#[tokio::test]
async fn malformed_performance_samples_fail_instead_of_exporting_rates() -> Result<()> {
    for profile in [
        MockRpcProfile::InvalidPerformanceTransactionSplit,
        MockRpcProfile::ZeroPerformanceSamplePeriod,
    ] {
        let (rpc_url, server_task) = start_mock_rpc_server_with_profile(profile).await?;
        let client = RpcClient::try_new(
            &rpc_transport(&rpc_url, 5),
            crate::watcher_state::RpcTelemetry::unobserved().for_connector("mock-rpc"),
        )?;

        assert!(client.get_cluster_performance_metrics().await.is_err());
        server_task.abort();
    }
    Ok(())
}

#[tokio::test]
async fn call_optional_surfaces_json_rpc_errors_from_http_400() -> Result<()> {
    let (rpc_url, server_task) = start_mock_rpc_server().await?;
    let mailbox = WatcherStateMailbox::new();
    let telemetry = mailbox.rpc_telemetry();
    let metrics = Arc::new(Metrics::new()?);
    let (state, actor) = mailbox.start("rpc-telemetry-test", Arc::clone(&metrics));
    let client = RpcClient::try_new(
        &rpc_transport(&rpc_url, 5),
        telemetry.for_connector("mock-rpc"),
    )?;
    drop(telemetry);
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
        let rendered = metrics.render()?;
        assert!(rendered.contains(
            "sol_rpc_request_duration_seconds_count{connector=\"mock-rpc\",method=\"getBlockProduction\",network=\"rpc-telemetry-test\"} 1"
        ));
        assert!(rendered.contains(
            "sol_rpc_request_failures_total{connector=\"mock-rpc\",method=\"getBlockProduction\",network=\"rpc-telemetry-test\"} 1"
        ));
        Ok(TaskExit::ShutdownRequested)
    });

    tasks.run().await
}

#[tokio::test]
async fn get_inflation_rewards_batches_requests_at_rpc_limit() -> Result<()> {
    let (rpc_url, server_task) = start_mock_rpc_server().await?;
    let client = RpcClient::try_new(
        &rpc_transport(&rpc_url, 5),
        crate::watcher_state::RpcTelemetry::unobserved().for_connector("mock-rpc"),
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
async fn get_account_balances_batches_targets_and_preserves_absence() -> Result<()> {
    let (rpc_url, server_task, methods) =
        start_recording_mock_rpc_server(MockRpcProfile::Default).await?;
    let client = RpcClient::try_new(
        &rpc_transport(&rpc_url, 5),
        crate::watcher_state::RpcTelemetry::unobserved().for_connector("mock-rpc"),
    )?;
    let addresses = (0..51)
        .map(|index| {
            if index == 50 {
                ("missing-id".to_string(), "missing-vote".to_string())
            } else {
                (format!("id-{index}"), format!("vote-{index}"))
            }
        })
        .collect::<Vec<_>>();
    let targets = addresses
        .iter()
        .map(|(identity, vote_account)| AccountBalanceTarget {
            identity,
            vote_account,
        })
        .collect::<Vec<_>>();

    let balances = client.get_account_balances(&targets).await?;
    let methods = methods.lock().await;
    let multiple_accounts_requests = methods
        .iter()
        .filter(|method| method.as_str() == "getMultipleAccounts")
        .count();
    let rent_requests = methods
        .iter()
        .filter(|method| method.as_str() == "getMinimumBalanceForRentExemption")
        .count();
    drop(methods);
    server_task.abort();

    assert_eq!(balances.len(), 51);
    assert_eq!(
        balances[0],
        Some(AccountBalances {
            identity_lamports: 3_000_000_000,
            vote_account_lamports: 2_500_000_000,
            vote_account_rent_exempt_minimum_lamports: Some(900_000_000),
        })
    );
    assert_eq!(balances[50], None);
    assert_eq!(multiple_accounts_requests, 2);
    assert_eq!(rent_requests, 1);
    Ok(())
}

#[tokio::test]
async fn missing_vote_account_space_preserves_balances_and_omits_vat() -> Result<()> {
    let (rpc_url, server_task) =
        start_mock_rpc_server_with_profile(MockRpcProfile::NoAccountSpace).await?;
    let client = RpcClient::try_new(
        &rpc_transport(&rpc_url, 5),
        crate::watcher_state::RpcTelemetry::unobserved().for_connector("mock-rpc"),
    )?;

    let targets = [AccountBalanceTarget {
        identity: "id-1",
        vote_account: "vote-1",
    }];
    let balances = client.get_account_balances(&targets).await?;
    server_task.abort();
    let balance = balances[0].context("missing account balances")?;
    assert_eq!(balance.identity_lamports, 3_000_000_000);
    assert_eq!(balance.vote_account_lamports, 2_500_000_000);
    assert_eq!(balance.vote_account_rent_exempt_minimum_lamports, None);

    let metrics = build_validator_funding_metrics(balance);
    assert_eq!(metrics.vat_reserve_lamports, None);
    assert_eq!(metrics.vat_epochs_available, None);
    assert_eq!(metrics.vat_next_epoch_funded, None);
    Ok(())
}
#[tokio::test]
async fn node_health_preserves_structured_catchup_distance() -> Result<()> {
    let (rpc_url, server_task) = start_mock_rpc_server_with_profile(MockRpcProfile::Behind).await?;
    let client = RpcClient::try_new(
        &rpc_transport(&rpc_url, 5),
        crate::watcher_state::RpcTelemetry::unobserved().for_connector("mock-rpc"),
    )?;

    let health = client.node_health().await?;
    server_task.abort();

    assert_eq!(
        health,
        crate::collector::NodeHealth::Behind { slots: Some(42) }
    );
    Ok(())
}

#[tokio::test]
async fn node_health_classifies_behind_error_before_http_status() -> Result<()> {
    let (rpc_url, server_task) =
        start_mock_rpc_server_with_profile(MockRpcProfile::BehindHttp503).await?;
    let client = RpcClient::try_new(
        &rpc_transport(&rpc_url, 5),
        crate::watcher_state::RpcTelemetry::unobserved().for_connector("mock-rpc"),
    )?;

    let health = client.node_health().await?;
    server_task.abort();

    assert_eq!(
        health,
        crate::collector::NodeHealth::Behind { slots: Some(42) }
    );
    Ok(())
}

#[tokio::test]
async fn rpc_health_errors_do_not_expose_secret_query_parameters() -> Result<()> {
    let (rpc_url, server_task) =
        start_mock_rpc_server_with_profile(MockRpcProfile::Unhealthy).await?;
    let secret = "do-not-expose-this-api-key";
    let client = RpcClient::try_new(
        &rpc_transport(&format!("{rpc_url}/?api-key={secret}"), 5),
        crate::watcher_state::RpcTelemetry::unobserved().for_connector("mock-rpc"),
    )?;

    let Err(error) = client.node_health().await else {
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
async fn rpc_transport_applies_custom_headers_without_exposing_credentials() -> Result<()> {
    let (rpc_url, server_task, _, authorization_headers) =
        start_header_recording_mock_rpc_server(MockRpcProfile::Default).await?;
    let secret = "rpc-header-secret";
    let transport: crate::config::RpcTransportConfig = toml::from_str(&format!(
        r#"
url = "{rpc_url}"
request_timeout_sec = 5
connect_timeout_sec = 5
headers = {{ authorization = "Bearer {secret}" }}
"#
    ))?;
    let debug = format!("{transport:?}");
    assert!(debug.contains("authorization"));
    assert!(!debug.contains(secret));
    assert!(!debug.contains(&rpc_url));
    let client = RpcClient::try_new(
        &transport,
        crate::watcher_state::RpcTelemetry::unobserved().for_connector("header-rpc"),
    )?;

    assert!(client.node_health().await?.is_healthy());
    server_task.abort();
    assert_eq!(
        authorization_headers.lock().await.as_slice(),
        &[Some(format!("Bearer {secret}"))]
    );
    Ok(())
}

#[tokio::test]
async fn rpc_transport_does_not_replay_custom_headers_across_redirects() -> Result<()> {
    let (rpc_url, redirect_server, target_server, redirected_headers) =
        start_redirecting_mock_rpc_server().await?;
    let secret = "redirect-header-secret";
    let transport: crate::config::RpcTransportConfig = toml::from_str(&format!(
        r#"
url = "{rpc_url}"
request_timeout_sec = 5
connect_timeout_sec = 5
headers = {{ "X-API-Key" = "{secret}" }}
"#
    ))?;
    let client = RpcClient::try_new(
        &transport,
        crate::watcher_state::RpcTelemetry::unobserved().for_connector("redirect-rpc"),
    )?;

    let result = client.node_health().await;
    redirect_server.abort();
    target_server.abort();

    assert!(result.is_err(), "RPC redirects must not be followed");
    assert!(
        redirected_headers.lock().await.is_empty(),
        "custom connector headers must never reach a redirected endpoint"
    );
    Ok(())
}

#[tokio::test]
async fn rpc_transport_sends_connector_commitment_on_wire() -> Result<()> {
    let (rpc_url, server_task, requests) =
        start_request_recording_mock_rpc_server(MockRpcProfile::Default).await?;
    let mut transport = rpc_transport(&rpc_url, 5);
    transport.commitment = "finalized".to_string();
    let client = RpcClient::try_new(
        &transport,
        crate::watcher_state::RpcTelemetry::unobserved().for_connector("finalized-rpc"),
    )?;

    let epoch_info = client.get_epoch_info().await;
    server_task.abort();
    epoch_info?;

    assert_eq!(
        requests.lock().await.as_slice(),
        &[serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getEpochInfo",
            "params": [{ "commitment": "finalized" }]
        })],
        "connector-local commitment must be sent in the JSON-RPC request"
    );
    Ok(())
}

#[tokio::test]
async fn rpc_transport_rejects_response_bodies_over_the_configured_limit() -> Result<()> {
    let (rpc_url, server_task) = start_mock_rpc_server().await?;
    let mut transport = rpc_transport(&rpc_url, 5);
    transport.response_size_limit_bytes = 8;
    let client = RpcClient::try_new(
        &transport,
        crate::watcher_state::RpcTelemetry::unobserved().for_connector("bounded-rpc"),
    )?;

    let Err(error) = client.node_health().await else {
        anyhow::bail!("the mock response must exceed the configured eight-byte limit");
    };
    server_task.abort();

    assert!(
        error
            .to_string()
            .contains("getHealth response exceeds configured 8 byte limit"),
        "{error:#}"
    );
    Ok(())
}

#[tokio::test]
async fn rpc_transport_enforces_total_request_timeout() -> Result<()> {
    let (rpc_url, server_task) =
        start_mock_rpc_server_with_profile(MockRpcProfile::Delayed).await?;
    let client = RpcClient::try_new(
        &rpc_transport(&rpc_url, 1),
        crate::watcher_state::RpcTelemetry::unobserved().for_connector("delayed-rpc"),
    )?;
    let started = std::time::Instant::now();

    let result = client.node_health().await;
    server_task.abort();

    assert!(result.is_err(), "delayed RPC request must time out");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "request deadline did not bound the delayed RPC call"
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
            rpc: Some(crate::config::RpcTransportConfig::new(format!(
                "{rpc_url}/?api-key={secret}"
            ))),
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
            rpc: Some(crate::config::RpcTransportConfig::new(rpc_url)),
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
            rpc: Some(crate::config::RpcTransportConfig::new(core_url)),
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
            "getBlockHeight",
            "getClusterNodes",
            "getEpochInfo",
            "getHealth",
            "getIdentity",
            "getLeaderSchedule",
            "getVersion",
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
            rpc: Some(crate::config::RpcTransportConfig::new(extended_url)),
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
            "getBlockHeight",
            "getBlockProduction",
            "getClusterNodes",
            "getEpochInfo",
            "getHealth",
            "getIdentity",
            "getInflationReward",
            "getLeaderSchedule",
            "getMinimumBalanceForRentExemption",
            "getMultipleAccounts",
            "getRecentPerformanceSamples",
            "getVersion",
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
            rpc: Some(crate::config::RpcTransportConfig::new(rpc_url)),
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
async fn connector_capability_probe_checks_declared_block_fee_rewards() -> Result<()> {
    let (rpc_url, server_task) =
        start_mock_rpc_server_with_profile(MockRpcProfile::NoBlockFeeRewards).await?;
    let config = Config {
        connectors: vec![ConnectorConfig {
            name: "block-reward-rpc".to_string(),
            provider: "mock-provider".to_string(),
            features: vec![ProviderFeature::BlockFeeRewards],
            solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
            rpc: Some(crate::config::RpcTransportConfig::new(rpc_url)),
            ..ConnectorConfig::default()
        }],
        ..Config::default()
    };

    let Err(error) =
        SourceChain::from_config(&config, crate::watcher_state::RpcTelemetry::unobserved())?
            .verify_capabilities()
            .await
    else {
        anyhow::bail!("missing getBlock support must fail the declared block-fee capability");
    };
    server_task.abort();

    let message = format!("{error:#}");
    assert!(
        message.contains("provider feature \"block-fee-rewards\""),
        "{message}"
    );
    assert!(message.contains("getBlock"), "{message}");
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
                rpc: Some(crate::config::RpcTransportConfig::new(rpc_url)),
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
                rpc: Some(crate::config::RpcTransportConfig::new(rpc_url)),
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
            rpc: Some(crate::config::RpcTransportConfig::new(rpc_url)),
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

type RedirectedHeaders = Arc<Mutex<Vec<Option<String>>>>;

async fn start_redirecting_mock_rpc_server(
) -> Result<(String, JoinHandle<()>, JoinHandle<()>, RedirectedHeaders)> {
    let target_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind redirect target listener")?;
    let target_address = target_listener
        .local_addr()
        .context("failed to read redirect target address")?;
    let redirected_headers = Arc::new(Mutex::new(Vec::new()));
    let target_app = Router::new()
        .route("/", post(redirected_rpc_target))
        .with_state(Arc::clone(&redirected_headers));
    let target_server = tokio::spawn(async move {
        let _ = axum::serve(target_listener, target_app).await;
    });

    let redirect_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind redirect source listener")?;
    let redirect_address = redirect_listener
        .local_addr()
        .context("failed to read redirect source address")?;
    let redirect_app = Router::new()
        .route("/", post(redirect_rpc_request))
        .with_state(format!("http://{target_address}"));
    let redirect_server = tokio::spawn(async move {
        let _ = axum::serve(redirect_listener, redirect_app).await;
    });

    Ok((
        format!("http://{redirect_address}"),
        redirect_server,
        target_server,
        redirected_headers,
    ))
}

async fn redirect_rpc_request(
    State(target_url): State<String>,
) -> (StatusCode, [(&'static str, String); 1]) {
    (StatusCode::TEMPORARY_REDIRECT, [("location", target_url)])
}

async fn redirected_rpc_target(
    State(headers_seen): State<RedirectedHeaders>,
    headers: HeaderMap,
    Json(_request): Json<Value>,
) -> Json<Value> {
    headers_seen.lock().await.push(
        headers
            .get("x-api-key")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string),
    );
    Json(rpc_success(json!("ok")))
}
