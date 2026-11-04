use super::{bool_label, Metrics};
use crate::{
    collector::{
        EconomicsMetrics, ModeStatus, ScopeMetrics, Snapshot, SnapshotDataSource, ValidatorStatus,
        WatchedValidatorMetrics,
    },
    config::{
        Config, ConnectorConfig, ConsensusCommands, ConsensusModeConfig, EconomicsCommands,
        EconomicsModeConfig, HealthCommands, HealthModeConfig, LeadershipCommands,
        LeadershipModeConfig, ModesConfig,
    },
    mode::{CommandId, ModeId, ProviderFeature, SolanaRpcCompatibility, Transport},
    source::SourceChain,
    watcher_state::RpcResponseCounts,
};
use anyhow::Result;
use std::{collections::BTreeMap, sync::Arc, time::Duration};

#[test]
fn renders_scoped_and_watched_validator_metrics() -> Result<()> {
    let metrics = Metrics::new()?;
    let snapshot = Snapshot {
        network: "mainnet-beta".to_string(),
        epoch: 42,
        slot: 1234,
        epoch_progress_pct: 34.5,
        epoch_slots_remaining: 65,
        average_slot_time_seconds: Some(0.4),
        estimated_epoch_end_unixtime: Some(1_800_000_000.0),
        vote_credits_sample_epoch: Some(41),
        reward_sample_epoch: Some(41),
        scopes: BTreeMap::from([(
            "scope:watched".to_string(),
            ScopeMetrics {
                current_count: 1,
                delinquent_count: 0,
                current_stake_lamports: 1_500_000_000,
                delinquent_stake_lamports: 0,
                latest_epoch_vote_credits: Some(11),
                latest_epoch_reward_lamports: 6_000_000_000,
                latest_epoch_rewarded_blocks_produced: 3,
                vote_efficiency_pct: Some(91.5),
                last_vote_slot_lag: 4.0,
                root_slot_lag: 5.0,
                average_commission_pct: 8.0,
                blocks_produced: 3,
                blocks_missed: 1,
                block_production_rate: 75.0,
                future_leader_slots: 2,
            },
        )]),
        watched_validators: vec![WatchedValidatorMetrics {
            identity: "id-1".to_string(),
            vote_account: "vote-1".to_string(),
            status: ValidatorStatus::Current,
            scopes: vec![
                "scope:all-network".to_string(),
                "scope:watched".to_string(),
                "region:sbg".to_string(),
            ],
            activated_stake_lamports: 1_500_000_000,
            latest_epoch_vote_credits: Some(11),
            latest_epoch_reward_lamports: 6_000_000_000,
            latest_epoch_rewarded_blocks_produced: 3,
            vote_efficiency_pct: Some(91.5),
            last_vote_slot_lag: 4,
            root_slot_lag: 5,
            commission_pct: 8,
            blocks_produced: 3,
            blocks_missed: 1,
            block_production_rate: 75.0,
            future_leader_slots: 2,
            next_leader_slot: Some(1300),
            slots_until_next_leader: Some(66),
            seconds_until_next_leader: Some(26.4),
        }],
        data_sources: vec![
            SnapshotDataSource {
                mode: ModeId::Consensus,
                command: CommandId::EpochInfo,
                connector: Arc::from("local-validator"),
                provider: Arc::from("solana-validator"),
                transport: Transport::Rpc,
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Core),
                feature: None,
                precedence: 1,
            },
            SnapshotDataSource {
                mode: ModeId::Economics,
                command: CommandId::InflationRewards,
                connector: Arc::from("helius-rpc"),
                provider: Arc::from("helius"),
                transport: Transport::Rpc,
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
                feature: Some(ProviderFeature::HistoricalData),
                precedence: 2,
            },
        ],
        mode_statuses: vec![
            ModeStatus {
                mode: ModeId::Consensus,
                up: true,
            },
            ModeStatus {
                mode: ModeId::Leadership,
                up: true,
            },
            ModeStatus {
                mode: ModeId::Economics,
                up: true,
            },
        ],
        economics: Some(EconomicsMetrics {
            identity: "id-1".to_string(),
            vote_account: "vote-1".to_string(),
            identity_balance_lamports: 3_000_000_000,
            vote_account_balance_lamports: 2_500_000_000,
            vote_account_rent_exempt_minimum_lamports: Some(900_000_000),
            vat_reserve_lamports: Some(1_600_000_000),
            vat_epochs_available: Some(2),
            vat_next_epoch_funded: Some(true),
        }),
    };
    metrics.set_watcher_up("mainnet-beta", true);
    metrics.set_rpc_up("mainnet-beta", true);
    metrics.set_ws_up("mainnet-beta", true);
    let mut config = Config {
        network: "mainnet-beta".to_string(),
        connectors: vec![
            ConnectorConfig {
                name: "local-validator".to_string(),
                provider: "solana-validator".to_string(),
                features: Vec::new(),
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Core),
                rpc_url: Some("http://127.0.0.1:8899".to_string()),
                ..ConnectorConfig::default()
            },
            ConnectorConfig {
                name: "helius-rpc".to_string(),
                provider: "helius".to_string(),
                features: vec![ProviderFeature::HistoricalData],
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
                rpc_url: Some("https://rpc.helius.example".to_string()),
                ..ConnectorConfig::default()
            },
            ConnectorConfig {
                name: "helius-ws".to_string(),
                provider: "helius".to_string(),
                features: Vec::new(),
                ws_url: Some("wss://ws.helius.example".to_string()),
                ..ConnectorConfig::default()
            },
            ConnectorConfig {
                name: "extended-rpc".to_string(),
                provider: "solana-validator".to_string(),
                features: vec![ProviderFeature::HistoricalData],
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
                rpc_url: Some("https://extended-rpc.example".to_string()),
                ..ConnectorConfig::default()
            },
        ],
        modes: modes_for(
            &["local-validator", "helius-rpc", "extended-rpc"],
            Some("helius-ws"),
            true,
        ),
        ..Config::default()
    };
    config.modes.health.commands.ws_health =
        vec!["local-validator".to_string(), "helius-ws".to_string()];
    let sources =
        SourceChain::from_config(&config, crate::watcher_state::RpcTelemetry::unobserved())?;
    metrics.begin_poll(&config, &sources);
    metrics.update_snapshot(&snapshot);
    let rendered = metrics.render()?;

    assert!(rendered.contains("sol_watcher_config_info{network=\"mainnet-beta\",transport_mode=\"poll-http-and-ws\",ws_configured=\"true\"} 1"));
    assert!(rendered.contains("sol_connector_info{connector=\"local-validator\",declaration_order=\"1\",has_rpc=\"true\",has_ws=\"false\",network=\"mainnet-beta\",provider=\"solana-validator\",solana_rpc_compatibility=\"core\"} 1"));
    assert!(rendered.contains("sol_connector_info{connector=\"helius-ws\",declaration_order=\"3\",has_rpc=\"false\",has_ws=\"true\",network=\"mainnet-beta\",provider=\"helius\",solana_rpc_compatibility=\"\"} 1"));
    assert!(rendered.contains("sol_connector_feature_info{connector=\"helius-rpc\",feature=\"historical-data\",network=\"mainnet-beta\",provider=\"helius\"} 1"));
    assert!(rendered.contains("sol_mode_command_route_info{available=\"true\",command=\"epoch-info\",connector=\"local-validator\",feature=\"\",mode=\"consensus\",network=\"mainnet-beta\",precedence=\"1\",provider=\"solana-validator\",required_solana_rpc_compatibility=\"core\",solana_rpc_compatibility=\"core\",transport=\"rpc\"} 1"));
    assert!(rendered.contains("sol_mode_command_route_info{available=\"false\",command=\"ws-health\",connector=\"local-validator\",feature=\"\",mode=\"health\",network=\"mainnet-beta\",precedence=\"1\",provider=\"solana-validator\",required_solana_rpc_compatibility=\"\",solana_rpc_compatibility=\"core\",transport=\"websocket\"} 1"));
    assert!(rendered.contains("sol_mode_command_route_info{available=\"true\",command=\"ws-health\",connector=\"helius-ws\",feature=\"\",mode=\"health\",network=\"mainnet-beta\",precedence=\"2\",provider=\"helius\",required_solana_rpc_compatibility=\"\",solana_rpc_compatibility=\"\",transport=\"websocket\"} 1"));
    assert!(rendered.contains("sol_mode_command_source_info{command=\"epoch-info\",connector=\"local-validator\",feature=\"\",mode=\"consensus\",network=\"mainnet-beta\",precedence=\"1\",provider=\"solana-validator\",required_solana_rpc_compatibility=\"core\",solana_rpc_compatibility=\"core\",transport=\"rpc\"} 1"));
    assert!(rendered.contains("sol_mode_command_source_info{command=\"inflation-rewards\",connector=\"helius-rpc\",feature=\"historical-data\",mode=\"economics\",network=\"mainnet-beta\",precedence=\"2\",provider=\"helius\",required_solana_rpc_compatibility=\"extended\",solana_rpc_compatibility=\"extended\",transport=\"rpc\"} 1"));
    assert!(rendered.contains("sol_mode_enabled{mode=\"economics\",network=\"mainnet-beta\"} 1"));
    assert!(rendered.contains("sol_mode_up{mode=\"leadership\",network=\"mainnet-beta\"} 1"));
    assert!(rendered.contains("sol_ws_up{network=\"mainnet-beta\"} 1"));
    assert!(rendered.contains("sol_vote_credits_sample_epoch{network=\"mainnet-beta\"} 41"));
    assert!(rendered.contains("sol_reward_sample_epoch{network=\"mainnet-beta\"} 41"));
    assert!(rendered.contains("sol_epoch_progress_pct{network=\"mainnet-beta\"} 34.5"));
    assert!(rendered.contains("sol_epoch_slots_remaining{network=\"mainnet-beta\"} 65"));
    assert!(rendered.contains("sol_average_slot_time_seconds{network=\"mainnet-beta\"} 0.4"));
    assert!(
        rendered.contains("sol_estimated_epoch_end_unixtime{network=\"mainnet-beta\"} 1800000000")
    );
    assert!(rendered.contains("sol_economics_identity_balance_sol{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 3"));
    assert!(rendered.contains("sol_economics_vote_account_balance_sol{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 2.5"));
    assert!(rendered.contains("sol_economics_vote_account_rent_exempt_minimum_sol{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 0.9"));
    assert!(rendered.contains("sol_economics_vat_reserve_sol{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 1.6"));
    assert!(rendered.contains("sol_economics_vat_epochs_available{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 2"));
    assert!(rendered.contains("sol_economics_vat_next_epoch_funded{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 1"));
    assert!(rendered.contains("sol_validator_status_count{network=\"mainnet-beta\",scope=\"scope:watched\",status=\"current\"} 1"));
    assert!(rendered
        .contains("sol_epoch_reward_sol{network=\"mainnet-beta\",scope=\"scope:watched\"} 6"));
    assert!(rendered
        .contains("sol_reward_per_block_sol{network=\"mainnet-beta\",scope=\"scope:watched\"} 2"));
    assert!(rendered.contains(
        "sol_vote_credits_latest_epoch{network=\"mainnet-beta\",scope=\"scope:watched\"} 11"
    ));
    assert!(rendered.contains(
        "sol_vote_efficiency_pct{network=\"mainnet-beta\",scope=\"scope:watched\"} 91.5"
    ));
    assert!(rendered.contains("sol_watched_validator_info{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 1"));
    assert!(rendered.contains("sol_watched_validator_scope_info{identity=\"id-1\",network=\"mainnet-beta\",scope=\"region:sbg\",vote_account=\"vote-1\"} 1"));
    assert!(rendered.contains("sol_watched_validator_activated_stake_sol{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 1.5"));
    assert!(rendered.contains("sol_watched_validator_vote_credits_latest_epoch{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 11"));
    assert!(rendered.contains("sol_watched_validator_vote_efficiency_pct{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 91.5"));
    assert!(rendered.contains("sol_watched_validator_epoch_reward_sol{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 6"));
    assert!(rendered.contains("sol_watched_validator_reward_per_block_sol{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 2"));
    assert!(rendered.contains("sol_watched_validator_block_production_rate{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 75"));
    assert!(rendered.contains("sol_watched_validator_next_leader_slot{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 1300"));
    assert!(rendered.contains("sol_watched_validator_slots_until_next_leader{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 66"));
    assert!(rendered.contains("sol_watched_validator_seconds_until_next_leader{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 26.4"));

    Ok(())
}

#[test]
fn economics_balances_render_when_rent_and_vat_are_unavailable() -> Result<()> {
    let metrics = Metrics::new()?;
    metrics.update_snapshot(&Snapshot {
        network: "mainnet-beta".to_string(),
        economics: Some(EconomicsMetrics {
            identity: "id-1".to_string(),
            vote_account: "vote-1".to_string(),
            identity_balance_lamports: 3_000_000_000,
            vote_account_balance_lamports: 2_500_000_000,
            vote_account_rent_exempt_minimum_lamports: None,
            vat_reserve_lamports: None,
            vat_epochs_available: None,
            vat_next_epoch_funded: None,
        }),
        ..Snapshot::default()
    });

    let rendered = metrics.render()?;
    assert!(rendered.contains("sol_economics_identity_balance_sol{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 3"));
    assert!(rendered.contains("sol_economics_vote_account_balance_sol{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 2.5"));
    assert!(!rendered.contains("sol_economics_vote_account_rent_exempt_minimum_sol{"));
    assert!(!rendered.contains("sol_economics_vat_reserve_sol{"));
    assert!(!rendered.contains("sol_economics_vat_epochs_available{"));
    assert!(!rendered.contains("sol_economics_vat_next_epoch_funded{"));
    Ok(())
}

#[test]
fn reset_removes_stale_watched_validator_metrics() -> Result<()> {
    let metrics = Metrics::new()?;
    let network = "mainnet-beta";

    let config = Config {
        network: network.to_string(),
        connectors: vec![ConnectorConfig {
            name: "primary".to_string(),
            provider: "solana".to_string(),
            features: Vec::new(),
            solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
            rpc_url: Some("https://api.mainnet-beta.solana.com".to_string()),
            ws_url: Some("wss://api.mainnet-beta.solana.com".to_string()),
            ..ConnectorConfig::default()
        }],
        modes: modes_for(&["primary"], Some("primary"), false),
        ..Config::default()
    };
    let sources =
        SourceChain::from_config(&config, crate::watcher_state::RpcTelemetry::unobserved())?;
    metrics.begin_poll(&config, &sources);
    metrics.update_snapshot(&Snapshot {
        network: network.to_string(),
        epoch: 1,
        slot: 1,
        epoch_progress_pct: 10.0,
        epoch_slots_remaining: 90,
        average_slot_time_seconds: Some(0.4),
        estimated_epoch_end_unixtime: Some(1_800_000_000.0),
        vote_credits_sample_epoch: Some(1),
        reward_sample_epoch: Some(1),
        scopes: BTreeMap::new(),
        watched_validators: vec![WatchedValidatorMetrics {
            identity: "old-id".to_string(),
            vote_account: "old-vote".to_string(),
            status: ValidatorStatus::Current,
            scopes: vec!["scope:watched".to_string()],
            activated_stake_lamports: 1_000_000_000,
            latest_epoch_vote_credits: Some(1),
            latest_epoch_reward_lamports: 1_000_000_000,
            latest_epoch_rewarded_blocks_produced: 1,
            vote_efficiency_pct: Some(100.0),
            last_vote_slot_lag: 0,
            root_slot_lag: 0,
            commission_pct: 0,
            blocks_produced: 1,
            blocks_missed: 0,
            block_production_rate: 100.0,
            future_leader_slots: 0,
            next_leader_slot: Some(10),
            slots_until_next_leader: Some(9),
            seconds_until_next_leader: Some(3.6),
        }],
        data_sources: vec![SnapshotDataSource {
            mode: ModeId::Consensus,
            command: CommandId::EpochInfo,
            connector: Arc::from("primary"),
            provider: Arc::from("solana"),
            transport: Transport::Rpc,
            solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
            feature: None,
            precedence: 1,
        }],
        mode_statuses: vec![ModeStatus {
            mode: ModeId::Consensus,
            up: true,
        }],
        economics: Some(EconomicsMetrics {
            identity: "old-id".to_string(),
            vote_account: "old-vote".to_string(),
            identity_balance_lamports: 1_000_000_000,
            vote_account_balance_lamports: 1_000_000_000,
            vote_account_rent_exempt_minimum_lamports: Some(500_000_000),
            vat_reserve_lamports: Some(500_000_000),
            vat_epochs_available: Some(0),
            vat_next_epoch_funded: Some(false),
        }),
    });
    metrics.update_snapshot(&Snapshot {
        network: network.to_string(),
        epoch: 2,
        slot: 2,
        epoch_progress_pct: 20.0,
        epoch_slots_remaining: 80,
        average_slot_time_seconds: None,
        estimated_epoch_end_unixtime: None,
        vote_credits_sample_epoch: None,
        reward_sample_epoch: None,
        scopes: BTreeMap::new(),
        watched_validators: vec![WatchedValidatorMetrics {
            identity: "current-id".to_string(),
            vote_account: "current-vote".to_string(),
            status: ValidatorStatus::Current,
            scopes: vec!["scope:watched".to_string()],
            activated_stake_lamports: 1_000_000_000,
            latest_epoch_vote_credits: None,
            latest_epoch_reward_lamports: 0,
            latest_epoch_rewarded_blocks_produced: 0,
            vote_efficiency_pct: None,
            last_vote_slot_lag: 0,
            root_slot_lag: 0,
            commission_pct: 0,
            blocks_produced: 0,
            blocks_missed: 0,
            block_production_rate: 0.0,
            future_leader_slots: 0,
            next_leader_slot: None,
            slots_until_next_leader: None,
            seconds_until_next_leader: None,
        }],
        data_sources: vec![],
        mode_statuses: vec![],
        economics: None,
    });

    let rendered = metrics.render()?;
    assert!(!rendered.contains("old-id"));
    assert!(!rendered.contains("old-vote"));
    assert!(rendered.contains("sol_watched_validator_info{identity=\"current-id\""));
    assert!(!rendered.contains("sol_vote_credits_sample_epoch{"));
    assert!(!rendered.contains("sol_reward_sample_epoch{"));
    assert!(!rendered.contains("sol_watched_validator_vote_credits_latest_epoch{"));
    assert!(!rendered.contains("sol_watched_validator_vote_efficiency_pct{"));
    assert!(!rendered.contains("sol_average_slot_time_seconds{"));
    assert!(!rendered.contains("sol_estimated_epoch_end_unixtime{"));
    assert!(!rendered.contains("sol_economics_vat_reserve_sol{"));

    Ok(())
}

#[test]
fn watcher_state_metrics_render_bounded_response_counts() -> Result<()> {
    let metrics = Metrics::new()?;
    metrics.update_watcher_state(
        "devnet",
        true,
        12,
        5,
        RpcResponseCounts {
            total: 18,
            successful_2xx: 12,
            redirection_3xx: 1,
            client_error_4xx: 2,
            server_error_5xx: 3,
            status_200: 11,
            status_403: 1,
            status_429: 1,
            status_500: 2,
            failed_attempts: 4,
            ..RpcResponseCounts::default()
        },
    );

    let rendered = metrics.render()?;
    for sample in [
        "sol_watcher_ready{network=\"devnet\"} 1",
        "sol_watcher_poll_round_count{network=\"devnet\"} 12",
        "sol_watcher_clean_poll_streak{network=\"devnet\"} 5",
        "sol_rpc_http_response_class_count{network=\"devnet\",status_class=\"2xx\"} 12",
        "sol_rpc_http_response_class_count{network=\"devnet\",status_class=\"3xx\"} 1",
        "sol_rpc_http_response_class_count{network=\"devnet\",status_class=\"4xx\"} 2",
        "sol_rpc_http_response_class_count{network=\"devnet\",status_class=\"5xx\"} 3",
        "sol_rpc_http_response_class_count{network=\"devnet\",status_class=\"other\"} 0",
        "sol_rpc_http_response_status_count{network=\"devnet\",status_code=\"200\"} 11",
        "sol_rpc_http_response_status_count{network=\"devnet\",status_code=\"403\"} 1",
        "sol_rpc_http_response_status_count{network=\"devnet\",status_code=\"429\"} 1",
        "sol_rpc_http_response_status_count{network=\"devnet\",status_code=\"500\"} 2",
        "sol_rpc_request_failure_count{network=\"devnet\"} 4",
    ] {
        assert!(rendered.contains(sample), "missing metric sample: {sample}");
    }

    metrics.update_watcher_state("mainnet-beta", false, 0, 0, RpcResponseCounts::default());
    let rendered = metrics.render()?;
    assert!(!rendered.contains("network=\"devnet\""));
    assert!(rendered.contains("sol_watcher_ready{network=\"mainnet-beta\"} 0"));

    Ok(())
}

#[test]
fn renders_and_resets_snapshot_freshness_and_rpc_request_metrics() -> Result<()> {
    let metrics = Metrics::new()?;
    metrics.reset_runtime_observations("devnet");
    metrics.record_snapshot_failure("devnet", Duration::from_millis(2_500));
    metrics.record_snapshot_success("devnet", Duration::from_millis(1_250), 1_800_000_000.5);
    metrics.record_rpc_request_duration(
        "devnet",
        "primary",
        "getEpochInfo",
        Duration::from_millis(420),
    );
    metrics.record_rpc_request_failure("devnet", "primary", "getEpochInfo");
    metrics.record_ws_probe_failure("devnet", "primary-ws");

    let rendered = metrics.render()?;
    for sample in [
        "sol_last_successful_snapshot_timestamp_seconds{network=\"devnet\"} 1800000000.5",
        "sol_snapshot_duration_seconds_count{network=\"devnet\",outcome=\"failure\"} 1",
        "sol_snapshot_duration_seconds_count{network=\"devnet\",outcome=\"success\"} 1",
        "sol_snapshot_failures_total{network=\"devnet\"} 1",
        "sol_rpc_request_duration_seconds_count{connector=\"primary\",method=\"getEpochInfo\",network=\"devnet\"} 1",
        "sol_rpc_request_failures_total{connector=\"primary\",method=\"getEpochInfo\",network=\"devnet\"} 1",
        "sol_ws_probe_failures_total{connector=\"primary-ws\",network=\"devnet\"} 1",
    ] {
        assert!(rendered.contains(sample), "missing metric sample: {sample}");
    }

    metrics.reset_runtime_observations("mainnet-beta");
    let rendered = metrics.render()?;
    assert!(!rendered.contains("network=\"devnet\""));
    assert!(
        rendered.contains("sol_snapshot_failures_total{network=\"mainnet-beta\"} 0"),
        "{rendered}"
    );

    Ok(())
}

#[test]
fn begin_poll_preserves_latest_mode_and_source_until_snapshot_replaces_source() -> Result<()> {
    let metrics = Metrics::new()?;
    let network = "mainnet-beta";
    let config = Config {
        network: network.to_string(),
        connectors: vec![
            ConnectorConfig {
                name: "primary".to_string(),
                provider: "solana".to_string(),
                features: Vec::new(),
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
                rpc_url: Some("https://primary.example".to_string()),
                ..ConnectorConfig::default()
            },
            ConnectorConfig {
                name: "fallback".to_string(),
                provider: "helius".to_string(),
                features: Vec::new(),
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
                rpc_url: Some("https://fallback.example".to_string()),
                ..ConnectorConfig::default()
            },
        ],
        modes: modes_for(&["primary", "fallback"], None, false),
        poll_interval_sec: 47,
        ..Config::default()
    };
    let sources =
        SourceChain::from_config(&config, crate::watcher_state::RpcTelemetry::unobserved())?;
    let source = |connector, provider, precedence| SnapshotDataSource {
        mode: ModeId::Consensus,
        command: CommandId::EpochInfo,
        connector: Arc::from(connector),
        provider: Arc::from(provider),
        transport: Transport::Rpc,
        solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
        feature: None,
        precedence,
    };

    metrics.begin_poll(&config, &sources);
    assert!(metrics
        .render()?
        .contains("sol_watcher_poll_interval_seconds{network=\"mainnet-beta\"} 47"));
    metrics.update_snapshot(&Snapshot {
        network: network.to_string(),
        data_sources: vec![source("primary", "solana", 1)],
        mode_statuses: vec![ModeStatus {
            mode: ModeId::Consensus,
            up: true,
        }],
        ..Snapshot::default()
    });
    assert!(metrics.render()?.contains("sol_mode_command_source_info{command=\"epoch-info\",connector=\"primary\",feature=\"\",mode=\"consensus\",network=\"mainnet-beta\",precedence=\"1\",provider=\"solana\",required_solana_rpc_compatibility=\"core\",solana_rpc_compatibility=\"extended\",transport=\"rpc\"} 1"));

    metrics.begin_poll(&config, &sources);
    let rendered = metrics.render()?;
    assert!(rendered.contains("sol_mode_command_source_info{command=\"epoch-info\",connector=\"primary\",feature=\"\",mode=\"consensus\",network=\"mainnet-beta\",precedence=\"1\",provider=\"solana\",required_solana_rpc_compatibility=\"core\",solana_rpc_compatibility=\"extended\",transport=\"rpc\"} 1"));
    assert!(rendered.contains("sol_mode_up{mode=\"consensus\",network=\"mainnet-beta\"} 1"));
    metrics.update_snapshot(&Snapshot {
        network: network.to_string(),
        data_sources: vec![source("fallback", "helius", 2)],
        ..Snapshot::default()
    });
    let rendered = metrics.render()?;
    assert!(!rendered.contains("sol_mode_command_source_info{command=\"epoch-info\",connector=\"primary\",feature=\"\",mode=\"consensus\",network=\"mainnet-beta\",precedence=\"1\",provider=\"solana\",required_solana_rpc_compatibility=\"core\",solana_rpc_compatibility=\"extended\",transport=\"rpc\"} 1"));
    assert!(rendered.contains("sol_mode_command_source_info{command=\"epoch-info\",connector=\"fallback\",feature=\"\",mode=\"consensus\",network=\"mainnet-beta\",precedence=\"2\",provider=\"helius\",required_solana_rpc_compatibility=\"core\",solana_rpc_compatibility=\"extended\",transport=\"rpc\"} 1"));
    assert!(!rendered.contains("sol_mode_up{mode=\"consensus\""));

    Ok(())
}

fn modes_for(
    rpc_connectors: &[&str],
    websocket_connector: Option<&str>,
    economics_enabled: bool,
) -> ModesConfig {
    let rpc_route = || {
        rpc_connectors
            .iter()
            .map(|name| (*name).to_string())
            .collect()
    };
    ModesConfig {
        health: HealthModeConfig {
            commands: HealthCommands {
                rpc_health: rpc_route(),
                ws_health: websocket_connector
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            },
        },
        consensus: ConsensusModeConfig {
            commands: ConsensusCommands {
                epoch_info: rpc_route(),
                vote_accounts: rpc_route(),
                recent_performance_samples: rpc_route(),
            },
        },
        leadership: LeadershipModeConfig {
            commands: LeadershipCommands {
                current_block_production: rpc_route(),
                current_leader_schedule: rpc_route(),
                next_leader_schedule: rpc_route(),
            },
        },
        economics: EconomicsModeConfig {
            enabled: economics_enabled,
            validator: None,
            commands: EconomicsCommands {
                account_balances: economics_enabled.then(rpc_route).unwrap_or_default(),
                inflation_rewards: economics_enabled.then(rpc_route).unwrap_or_default(),
                reward_epoch_block_production: economics_enabled
                    .then(rpc_route)
                    .unwrap_or_default(),
            },
        },
    }
}

#[test]
fn helper_labels_return_expected_values() {
    assert_eq!(ValidatorStatus::Current.as_str(), "current");
    assert_eq!(ValidatorStatus::Delinquent.as_str(), "delinquent");
    assert_eq!(bool_label(true), "true");
    assert_eq!(bool_label(false), "false");
}
