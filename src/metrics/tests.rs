use super::{bool_label, Metrics, BUILD_REVISION};
use crate::{
    collector::{
        ClusterPerformanceMetrics, EconomicsMetrics, GossipObservation, JitoMevReward, ModeStatus,
        NodeHealth, NodeMetrics, NodeVotingStatus, RecentBlockFeeRewards, ScopeMetrics, Snapshot,
        SnapshotDataSource, TrilliumBlockRewards, TrilliumMevCapture, TrilliumTransactionMetrics,
        ValidatorFundingMetrics, ValidatorStatus, WatchedValidatorMetrics,
    },
    config::{
        Config, ConnectorConfig, ConsensusCommands, ConsensusModeConfig, EconomicsCommands,
        EconomicsModeConfig, HealthCommands, HealthModeConfig, LeadershipCommands,
        LeadershipModeConfig, ModesConfig,
    },
    mode::{CommandId, ModeId, ProviderFeature, SolanaRpcCompatibility, Transport},
    source::SourceChain,
    watcher_state::{RpcResponseCounts, WatcherStateSnapshot},
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
        epoch_slots: 100,
        epoch_progress_pct: 34.5,
        epoch_slots_remaining: 65,
        cluster_performance: Some(ClusterPerformanceMetrics {
            average_slot_time_seconds: Some(0.4),
            transactions_per_second: 25.0,
            vote_transactions_per_second: Some(22.5),
            non_vote_transactions_per_second: Some(2.5),
            slots_per_second: 2.5,
        }),
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
                latest_epoch_reward_lamports: Some(6_000_000_000),
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
            gossip: GossipObservation::Present {
                version: Some("3.1.8".to_string()),
                feature_set: Some(2_891_131_721),
                shred_version: Some(56_123),
            },
            scopes: vec![
                "scope:all-network".to_string(),
                "scope:watched".to_string(),
                "region:sbg".to_string(),
            ],
            activated_stake_lamports: 1_500_000_000,
            funding: Some(ValidatorFundingMetrics {
                identity_balance_lamports: 3_000_000_000,
                vote_account_balance_lamports: 2_500_000_000,
                vote_account_rent_exempt_minimum_lamports: Some(900_000_000),
                vat_reserve_lamports: Some(1_600_000_000),
                vat_epochs_available: Some(2),
                vat_next_epoch_funded: Some(true),
            }),
            latest_epoch_vote_credits: Some(11),
            latest_epoch_reward_lamports: Some(6_000_000_000),
            latest_epoch_rewarded_blocks_produced: 3,
            vote_efficiency_pct: Some(91.5),
            last_vote_slot_lag: 4,
            root_slot_lag: 5,
            commission_pct: 8,
            blocks_produced: 3,
            blocks_missed: 1,
            block_production_rate: 75.0,
            current_epoch_scheduled_leader_slots: Some(10),
            future_leader_slots: 2,
            next_leader_slot: Some(1300),
            slots_until_next_leader: Some(66),
            seconds_until_next_leader: Some(26.4),
            upcoming_leader_slots: vec![1300, 1320],
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
            SnapshotDataSource {
                mode: ModeId::Economics,
                command: CommandId::RecentBlockFeeRewards,
                connector: Arc::from("helius-rpc"),
                provider: Arc::from("helius"),
                transport: Transport::Rpc,
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
                feature: Some(ProviderFeature::BlockFeeRewards),
                precedence: 1,
            },
            SnapshotDataSource {
                mode: ModeId::Economics,
                command: CommandId::JitoMevRewards,
                connector: Arc::from("jito-api"),
                provider: Arc::from("jito"),
                transport: Transport::ProviderApi,
                solana_rpc_compatibility: None,
                feature: Some(ProviderFeature::JitoMevRewards),
                precedence: 1,
            },
            SnapshotDataSource {
                mode: ModeId::Economics,
                command: CommandId::TrilliumMevCapture,
                connector: Arc::from("trillium-api"),
                provider: Arc::from("trillium"),
                transport: Transport::ProviderApi,
                solana_rpc_compatibility: None,
                feature: Some(ProviderFeature::TrilliumMevCapture),
                precedence: 1,
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
        node: Some(NodeMetrics {
            identity: "id-1".to_string(),
            version: "3.1.8".to_string(),
            feature_set: Some(2_891_131_721),
            block_height: 1_233,
            health: NodeHealth::Healthy,
            voting_status: NodeVotingStatus::Current,
        }),
        economics: Some(EconomicsMetrics {
            identity: "id-1".to_string(),
            vote_account: "vote-1".to_string(),
            identity_balance_lamports: 3_000_000_000,
            vote_account_balance_lamports: 2_500_000_000,
            vote_account_rent_exempt_minimum_lamports: Some(900_000_000),
            vat_reserve_lamports: Some(1_600_000_000),
            vat_epochs_available: Some(2),
            vat_next_epoch_funded: Some(true),
            jito_mev_reward: Some(JitoMevReward {
                epoch: 41,
                reward_lamports: 123_456_789,
                commission_bps: 800,
            }),
            trillium_mev_capture: Some(TrilliumMevCapture {
                epoch: 41,
                captured_sol: Some(1.25),
                validator_sol: Some(0.25),
                stakers_sol: Some(0.9),
                block_engine_sol: Some(0.05),
                tip_router_sol: Some(0.05),
                per_leader_slot_sol: Some(0.0125),
                commission_pct: Some(20.0),
                total_jito_tips_sol: Some(1.3),
                total_jito_tip_count: Some(125),
                leader_slots: 100,
                blocks_produced: 98,
                activated_stake_sol: Some(200_000.0),
                block_rewards: TrilliumBlockRewards {
                    after_burn_sol: Some(2.1),
                    priority_fees_sol: Some(1.5),
                    validator_priority_fees_sol: Some(1.2),
                    delegator_priority_fees_sol: Some(0.3),
                    validator_signature_fees_sol: Some(0.2),
                    average_mev_per_block_sol: Some(0.012),
                    median_mev_per_block_sol: Some(0.009),
                    average_priority_fees_per_block_sol: Some(0.015),
                    median_priority_fees_per_block_sol: Some(0.011),
                    average_signature_fees_per_block_sol: Some(0.002),
                    median_signature_fees_per_block_sol: Some(0.001),
                    average_jito_tips_per_block_sol: Some(0.013),
                    median_jito_tips_per_block_sol: Some(0.008),
                    priority_fee_commission_pct: Some(15.0),
                },
                transactions: TrilliumTransactionMetrics {
                    successful: Some(8_000),
                    failed: Some(2_000),
                    successful_user: Some(6_000),
                    failed_user: Some(2_000),
                    average_per_block: Some(100.0),
                    average_user_per_block: Some(80.0),
                    average_vote_per_block: Some(20.0),
                    median_per_block: Some(90.0),
                    median_user_per_block: Some(70.0),
                    average_success_pct: Some(80.0),
                    average_user_success_pct: Some(75.0),
                    average_compute_units_per_block: Some(14_000_000.0),
                    median_compute_units_per_block: Some(13_000_000.0),
                },
            }),
            recent_block_fee_rewards: Some(RecentBlockFeeRewards {
                latest_slot: 1_230,
                latest_reward_lamports: 2_000_000,
                total_reward_lamports: 6_000_000,
                sampled_block_count: 3,
            }),
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
                rpc: Some(crate::config::RpcTransportConfig::new(
                    "http://127.0.0.1:8899".to_string(),
                )),
                ..ConnectorConfig::default()
            },
            ConnectorConfig {
                name: "helius-rpc".to_string(),
                provider: "helius".to_string(),
                features: vec![
                    ProviderFeature::HistoricalData,
                    ProviderFeature::BlockFeeRewards,
                ],
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
                rpc: Some(crate::config::RpcTransportConfig::new(
                    "https://rpc.helius.example".to_string(),
                )),
                ..ConnectorConfig::default()
            },
            ConnectorConfig {
                name: "helius-ws".to_string(),
                provider: "helius".to_string(),
                features: Vec::new(),
                websocket: Some(crate::config::WebsocketTransportConfig::new(
                    "wss://ws.helius.example".to_string(),
                )),
                ..ConnectorConfig::default()
            },
            ConnectorConfig {
                name: "extended-rpc".to_string(),
                provider: "solana-validator".to_string(),
                features: vec![ProviderFeature::HistoricalData],
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
                rpc: Some(crate::config::RpcTransportConfig::new(
                    "https://extended-rpc.example".to_string(),
                )),
                ..ConnectorConfig::default()
            },
            ConnectorConfig {
                name: "jito-api".to_string(),
                provider: "jito".to_string(),
                features: vec![ProviderFeature::JitoMevRewards],
                provider_api: Some(crate::config::HttpTransportConfig::new(
                    "https://kobe.mainnet.jito.network/api/v1".to_string(),
                )),
                ..ConnectorConfig::default()
            },
            ConnectorConfig {
                name: "trillium-api".to_string(),
                provider: "trillium".to_string(),
                features: vec![ProviderFeature::TrilliumMevCapture],
                provider_api: Some(crate::config::HttpTransportConfig::new(
                    "https://api.trillium.so".to_string(),
                )),
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
    config.fingerprint = "abc123".to_string();
    config.modes.health.commands.ws_health =
        vec!["local-validator".to_string(), "helius-ws".to_string()];
    config.modes.economics.commands.jito_mev_rewards = vec!["jito-api".to_string()];
    config.modes.economics.commands.trillium_mev_capture = vec!["trillium-api".to_string()];
    config.modes.economics.commands.recent_block_fee_rewards = vec!["helius-rpc".to_string()];
    let sources = SourceChain::from_config(
        &config,
        crate::watcher_state::RuntimeTelemetry::unobserved(),
    )?;
    metrics.begin_poll(&config, &sources);
    metrics.update_snapshot(&snapshot);
    metrics.record_config_reload("mainnet-beta", "accepted");
    metrics.record_config_reload("mainnet-beta", "rejected");
    let rendered = metrics.render()?;

    assert!(rendered.contains(&format!(
        "sol_watcher_build_info{{revision=\"{BUILD_REVISION}\",version=\"{}\"}} 1",
        env!("CARGO_PKG_VERSION")
    )));
    assert!(rendered.contains("sol_config_info{fingerprint=\"abc123\",network=\"mainnet-beta\"} 1"));
    assert!(rendered
        .contains("sol_config_reload_total{network=\"mainnet-beta\",outcome=\"accepted\"} 1"));
    assert!(rendered
        .contains("sol_config_reload_total{network=\"mainnet-beta\",outcome=\"rejected\"} 1"));
    assert!(rendered.contains("sol_mode_command_fallback_active{command=\"inflation-rewards\",connector=\"helius-rpc\",mode=\"economics\",network=\"mainnet-beta\"} 1"));
    assert!(rendered.contains("sol_watcher_config_info{network=\"mainnet-beta\",transport_mode=\"poll-http-and-ws\",ws_configured=\"true\"} 1"));
    assert!(rendered.contains("sol_connector_info{connector=\"local-validator\",declaration_order=\"1\",has_provider_api=\"false\",has_rpc=\"true\",has_ws=\"false\",network=\"mainnet-beta\",provider=\"solana-validator\",solana_rpc_compatibility=\"core\"} 1"));
    assert!(rendered.contains("sol_connector_info{connector=\"helius-ws\",declaration_order=\"3\",has_provider_api=\"false\",has_rpc=\"false\",has_ws=\"true\",network=\"mainnet-beta\",provider=\"helius\",solana_rpc_compatibility=\"\"} 1"));
    assert!(rendered.contains("sol_connector_feature_info{connector=\"helius-rpc\",feature=\"historical-data\",network=\"mainnet-beta\",provider=\"helius\"} 1"));
    assert!(rendered.contains("sol_connector_info{connector=\"jito-api\",declaration_order=\"5\",has_provider_api=\"true\",has_rpc=\"false\",has_ws=\"false\",network=\"mainnet-beta\",provider=\"jito\",solana_rpc_compatibility=\"\"} 1"));
    assert!(rendered.contains("sol_connector_feature_info{connector=\"jito-api\",feature=\"jito-mev-rewards\",network=\"mainnet-beta\",provider=\"jito\"} 1"));
    assert!(rendered.contains("sol_mode_command_route_info{available=\"true\",command=\"jito-mev-rewards\",connector=\"jito-api\",feature=\"jito-mev-rewards\",mode=\"economics\",network=\"mainnet-beta\",precedence=\"1\",provider=\"jito\",required_solana_rpc_compatibility=\"\",solana_rpc_compatibility=\"\",transport=\"provider-api\"} 1"));
    assert!(rendered.contains("sol_connector_info{connector=\"trillium-api\",declaration_order=\"6\",has_provider_api=\"true\",has_rpc=\"false\",has_ws=\"false\",network=\"mainnet-beta\",provider=\"trillium\",solana_rpc_compatibility=\"\"} 1"));
    assert!(rendered.contains("sol_connector_feature_info{connector=\"trillium-api\",feature=\"trillium-mev-capture\",network=\"mainnet-beta\",provider=\"trillium\"} 1"));
    assert!(rendered.contains("sol_mode_command_route_info{available=\"true\",command=\"trillium-mev-capture\",connector=\"trillium-api\",feature=\"trillium-mev-capture\",mode=\"economics\",network=\"mainnet-beta\",precedence=\"1\",provider=\"trillium\",required_solana_rpc_compatibility=\"\",solana_rpc_compatibility=\"\",transport=\"provider-api\"} 1"));
    assert!(rendered.contains("sol_connector_feature_info{connector=\"helius-rpc\",feature=\"block-fee-rewards\",network=\"mainnet-beta\",provider=\"helius\"} 1"));
    assert!(rendered.contains("sol_mode_command_route_info{available=\"true\",command=\"recent-block-fee-rewards\",connector=\"helius-rpc\",feature=\"block-fee-rewards\",mode=\"economics\",network=\"mainnet-beta\",precedence=\"1\",provider=\"helius\",required_solana_rpc_compatibility=\"extended\",solana_rpc_compatibility=\"extended\",transport=\"rpc\"} 1"));
    assert!(rendered.contains("sol_mode_command_route_info{available=\"true\",command=\"epoch-info\",connector=\"local-validator\",feature=\"\",mode=\"consensus\",network=\"mainnet-beta\",precedence=\"1\",provider=\"solana-validator\",required_solana_rpc_compatibility=\"core\",solana_rpc_compatibility=\"core\",transport=\"rpc\"} 1"));
    assert!(rendered.contains("sol_mode_command_route_info{available=\"false\",command=\"ws-health\",connector=\"local-validator\",feature=\"\",mode=\"health\",network=\"mainnet-beta\",precedence=\"1\",provider=\"solana-validator\",required_solana_rpc_compatibility=\"\",solana_rpc_compatibility=\"core\",transport=\"websocket\"} 1"));
    assert!(rendered.contains("sol_mode_command_route_info{available=\"true\",command=\"ws-health\",connector=\"helius-ws\",feature=\"\",mode=\"health\",network=\"mainnet-beta\",precedence=\"2\",provider=\"helius\",required_solana_rpc_compatibility=\"\",solana_rpc_compatibility=\"\",transport=\"websocket\"} 1"));
    assert!(rendered.contains("sol_mode_command_source_info{command=\"epoch-info\",connector=\"local-validator\",feature=\"\",mode=\"consensus\",network=\"mainnet-beta\",precedence=\"1\",provider=\"solana-validator\",required_solana_rpc_compatibility=\"core\",solana_rpc_compatibility=\"core\",transport=\"rpc\"} 1"));
    assert!(rendered.contains("sol_mode_command_source_info{command=\"inflation-rewards\",connector=\"helius-rpc\",feature=\"historical-data\",mode=\"economics\",network=\"mainnet-beta\",precedence=\"2\",provider=\"helius\",required_solana_rpc_compatibility=\"extended\",solana_rpc_compatibility=\"extended\",transport=\"rpc\"} 1"));
    assert!(rendered.contains("sol_mode_command_source_info{command=\"jito-mev-rewards\",connector=\"jito-api\",feature=\"jito-mev-rewards\",mode=\"economics\",network=\"mainnet-beta\",precedence=\"1\",provider=\"jito\",required_solana_rpc_compatibility=\"\",solana_rpc_compatibility=\"\",transport=\"provider-api\"} 1"));
    assert!(rendered.contains("sol_mode_command_source_info{command=\"trillium-mev-capture\",connector=\"trillium-api\",feature=\"trillium-mev-capture\",mode=\"economics\",network=\"mainnet-beta\",precedence=\"1\",provider=\"trillium\",required_solana_rpc_compatibility=\"\",solana_rpc_compatibility=\"\",transport=\"provider-api\"} 1"));
    assert!(rendered.contains("sol_mode_command_source_info{command=\"recent-block-fee-rewards\",connector=\"helius-rpc\",feature=\"block-fee-rewards\",mode=\"economics\",network=\"mainnet-beta\",precedence=\"1\",provider=\"helius\",required_solana_rpc_compatibility=\"extended\",solana_rpc_compatibility=\"extended\",transport=\"rpc\"} 1"));
    assert!(rendered.contains("sol_mode_enabled{mode=\"economics\",network=\"mainnet-beta\"} 1"));
    assert!(rendered.contains("sol_mode_up{mode=\"leadership\",network=\"mainnet-beta\"} 1"));
    assert!(rendered.contains("sol_ws_up{network=\"mainnet-beta\"} 1"));
    assert!(rendered.contains("sol_vote_credits_sample_epoch{network=\"mainnet-beta\"} 41"));
    assert!(rendered.contains("sol_reward_sample_epoch{network=\"mainnet-beta\"} 41"));
    assert!(rendered.contains("sol_epoch_progress_pct{network=\"mainnet-beta\"} 34.5"));
    assert!(rendered.contains("sol_epoch_slots_remaining{network=\"mainnet-beta\"} 65"));
    assert!(rendered.contains("sol_average_slot_time_seconds{network=\"mainnet-beta\"} 0.4"));
    assert!(rendered.contains("sol_cluster_transactions_per_second{network=\"mainnet-beta\"} 25"));
    assert!(rendered
        .contains("sol_cluster_vote_transactions_per_second{network=\"mainnet-beta\"} 22.5"));
    assert!(rendered
        .contains("sol_cluster_non_vote_transactions_per_second{network=\"mainnet-beta\"} 2.5"));
    assert!(rendered.contains("sol_cluster_slots_per_second{network=\"mainnet-beta\"} 2.5"));
    assert!(
        rendered.contains("sol_estimated_epoch_end_unixtime{network=\"mainnet-beta\"} 1800000000")
    );
    assert!(rendered.contains("sol_economics_identity_balance_sol{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 3"));
    assert!(rendered.contains("sol_economics_vote_account_balance_sol{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 2.5"));
    assert!(rendered.contains("sol_economics_vote_account_rent_exempt_minimum_sol{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 0.9"));
    assert!(rendered.contains("sol_economics_vat_reserve_sol{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 1.6"));
    assert!(rendered.contains("sol_economics_vat_epochs_available{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 2"));
    assert!(rendered.contains("sol_economics_vat_next_epoch_funded{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 1"));
    assert!(rendered.contains("sol_watched_validator_identity_balance_sol{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 3"));
    assert!(rendered.contains("sol_watched_validator_vote_account_balance_sol{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 2.5"));
    assert!(rendered.contains("sol_watched_validator_vote_account_rent_exempt_minimum_sol{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 0.9"));
    assert!(rendered.contains("sol_watched_validator_vat_reserve_sol{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 1.6"));
    assert!(rendered.contains("sol_watched_validator_vat_epochs_available{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 2"));
    assert!(rendered.contains("sol_watched_validator_vat_next_epoch_funded{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 1"));
    assert!(rendered.contains("sol_economics_block_fee_reward_sol{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 0.002"));
    assert!(rendered.contains("sol_economics_block_fee_reward_slot{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 1230"));
    assert!(rendered.contains("sol_economics_recent_block_fee_reward_average_sol{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 0.002"));
    assert!(rendered.contains("sol_economics_recent_block_fee_reward_sample_count{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 3"));
    assert!(rendered.contains("sol_economics_jito_mev_reward_sol{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 0.123456789"));
    assert!(rendered.contains("sol_economics_jito_mev_commission_pct{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 8"));
    assert!(rendered.contains("sol_economics_jito_mev_reward_epoch{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 41"));
    assert!(rendered.contains("sol_economics_trillium_mev_capture_sol{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 1.25"));
    assert!(rendered.contains("sol_economics_trillium_mev_allocation_sol{destination=\"validator\",identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 0.25"));
    assert!(rendered.contains("sol_economics_trillium_mev_allocation_sol{destination=\"stakers\",identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 0.9"));
    assert!(rendered.contains("sol_economics_trillium_mev_allocation_sol{destination=\"jito-block-engine\",identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 0.05"));
    assert!(rendered.contains("sol_economics_trillium_mev_allocation_sol{destination=\"jito-tip-router\",identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 0.05"));
    assert!(rendered.contains("sol_economics_trillium_mev_per_leader_slot_sol{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 0.0125"));
    assert!(rendered.contains("sol_economics_trillium_mev_commission_pct{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 20"));
    assert!(rendered.contains("sol_economics_trillium_total_jito_tips_sol{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 1.3"));
    assert!(rendered.contains("sol_economics_trillium_jito_tip_count{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 125"));
    assert!(rendered.contains("sol_economics_trillium_leader_slots{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 100"));
    assert!(rendered.contains("sol_economics_trillium_blocks_produced{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 98"));
    assert!(rendered.contains("sol_economics_trillium_mev_capture_epoch{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 41"));
    assert!(rendered.contains("sol_economics_trillium_block_reward_sol{component=\"after-burn\",identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 2.1"));
    assert!(rendered.contains("sol_economics_trillium_block_reward_sol{component=\"priority-fees-total\",identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 1.5"));
    assert!(rendered.contains("sol_economics_trillium_per_block_reward_sol{component=\"mev\",identity=\"id-1\",network=\"mainnet-beta\",statistic=\"average\",vote_account=\"vote-1\"} 0.012"));
    assert!(rendered.contains("sol_economics_trillium_transactions{identity=\"id-1\",network=\"mainnet-beta\",outcome=\"successful\",transaction_type=\"all\",vote_account=\"vote-1\"} 8000"));
    assert!(rendered.contains("sol_economics_trillium_transactions_per_block{identity=\"id-1\",network=\"mainnet-beta\",statistic=\"average\",transaction_type=\"all\",vote_account=\"vote-1\"} 100"));
    assert!(rendered.contains("sol_economics_trillium_transaction_success_pct{identity=\"id-1\",network=\"mainnet-beta\",transaction_type=\"all\",vote_account=\"vote-1\"} 80"));
    assert!(rendered.contains("sol_economics_trillium_compute_units_per_block{identity=\"id-1\",network=\"mainnet-beta\",statistic=\"average\",vote_account=\"vote-1\"} 14000000"));
    assert!(rendered.contains("sol_economics_trillium_activated_stake_sol{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 200000"));
    assert!(rendered.contains("sol_economics_trillium_priority_fee_commission_pct{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 15"));
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
    assert!(rendered.contains("sol_watched_validator_gossip_visible{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 1"));
    assert!(rendered.contains("sol_watched_validator_gossip_info{feature_set=\"2891131721\",identity=\"id-1\",network=\"mainnet-beta\",shred_version=\"56123\",status=\"current\",version=\"3.1.8\",vote_account=\"vote-1\"} 1"));
    assert!(rendered.contains("sol_watched_validator_scope_info{identity=\"id-1\",network=\"mainnet-beta\",scope=\"region:sbg\",vote_account=\"vote-1\"} 1"));
    assert!(rendered.contains("sol_watched_validator_activated_stake_sol{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 1.5"));
    assert!(rendered.contains("sol_watched_validator_vote_credits_latest_epoch{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 11"));
    assert!(rendered.contains("sol_watched_validator_vote_efficiency_pct{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 91.5"));
    assert!(rendered.contains("sol_watched_validator_epoch_reward_sol{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 6"));
    assert!(rendered.contains("sol_watched_validator_reward_per_block_sol{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 2"));
    assert!(rendered.contains("sol_watched_validator_block_production_rate{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 75"));
    assert!(rendered.contains("sol_watched_validator_current_epoch_scheduled_leader_slots{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 10"));
    assert!(rendered.contains("sol_watched_validator_scheduled_leader_slots_per_hour{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 900"));
    assert!(rendered.contains("sol_watched_validator_upcoming_leader_slot{identity=\"id-1\",network=\"mainnet-beta\",position=\"01\",status=\"current\",vote_account=\"vote-1\"} 1300"));
    assert!(rendered.contains("sol_watched_validator_upcoming_leader_slot{identity=\"id-1\",network=\"mainnet-beta\",position=\"02\",status=\"current\",vote_account=\"vote-1\"} 1320"));
    assert!(rendered.contains("sol_watched_validator_next_leader_slot{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 1300"));
    assert!(rendered.contains("sol_watched_validator_slots_until_next_leader{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 66"));
    assert!(rendered.contains("sol_watched_validator_seconds_until_next_leader{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 26.4"));
    assert!(rendered.contains("sol_node_info{feature_set=\"2891131721\",identity=\"id-1\",network=\"mainnet-beta\",version=\"3.1.8\",voting_status=\"current\"} 1"));
    assert!(rendered.contains("sol_node_health{identity=\"id-1\",network=\"mainnet-beta\"} 1"));
    assert!(rendered
        .contains("sol_node_catchup_distance_slots{identity=\"id-1\",network=\"mainnet-beta\"} 0"));
    assert!(
        rendered.contains("sol_node_block_height{identity=\"id-1\",network=\"mainnet-beta\"} 1233")
    );

    Ok(())
}

#[test]
fn scheduled_frequency_requires_a_valid_epoch_duration() -> Result<()> {
    let metrics = Metrics::new()?;
    let validator = WatchedValidatorMetrics {
        identity: "id-1".to_string(),
        vote_account: "vote-1".to_string(),
        status: ValidatorStatus::Current,
        gossip: GossipObservation::Unavailable,
        scopes: vec!["scope:watched".to_string()],
        activated_stake_lamports: 1,
        funding: None,
        latest_epoch_vote_credits: None,
        vote_efficiency_pct: None,
        last_vote_slot_lag: 0,
        root_slot_lag: 0,
        commission_pct: 0,
        latest_epoch_reward_lamports: None,
        latest_epoch_rewarded_blocks_produced: 0,
        blocks_produced: 0,
        blocks_missed: 0,
        block_production_rate: 0.0,
        current_epoch_scheduled_leader_slots: Some(10),
        future_leader_slots: 0,
        next_leader_slot: None,
        slots_until_next_leader: None,
        seconds_until_next_leader: None,
        upcoming_leader_slots: vec![],
    };
    let snapshot = |epoch_slots, average_slot_time_seconds| Snapshot {
        network: "mainnet-beta".to_string(),
        epoch_slots,
        cluster_performance: Some(ClusterPerformanceMetrics {
            average_slot_time_seconds: Some(average_slot_time_seconds),
            transactions_per_second: 0.0,
            vote_transactions_per_second: None,
            non_vote_transactions_per_second: None,
            slots_per_second: 0.0,
        }),
        watched_validators: vec![validator.clone()],
        ..Snapshot::default()
    };

    metrics.update_snapshot(&snapshot(0, 0.4));
    let zero_epoch = metrics.render()?;
    assert!(zero_epoch
        .contains("sol_watched_validator_current_epoch_scheduled_leader_slots{identity=\"id-1\""));
    assert!(!zero_epoch.contains("sol_watched_validator_scheduled_leader_slots_per_hour{"));

    metrics.update_snapshot(&snapshot(100, f64::NAN));
    let invalid_slot_time = metrics.render()?;
    assert!(!invalid_slot_time.contains("sol_watched_validator_scheduled_leader_slots_per_hour{"));
    Ok(())
}

#[test]
fn watched_validator_balances_render_for_every_validator_and_preserve_absence() -> Result<()> {
    let metrics = Metrics::new()?;
    let watched_validator =
        |identity: &str, vote_account: &str, funding: ValidatorFundingMetrics| {
            WatchedValidatorMetrics {
                identity: identity.to_string(),
                vote_account: vote_account.to_string(),
                status: ValidatorStatus::Current,
                gossip: GossipObservation::Unavailable,
                scopes: vec!["scope:watched".to_string()],
                activated_stake_lamports: 0,
                funding: Some(funding),
                latest_epoch_vote_credits: None,
                vote_efficiency_pct: None,
                last_vote_slot_lag: 0,
                root_slot_lag: 0,
                commission_pct: 0,
                latest_epoch_reward_lamports: None,
                latest_epoch_rewarded_blocks_produced: 0,
                blocks_produced: 0,
                blocks_missed: 0,
                block_production_rate: 0.0,
                current_epoch_scheduled_leader_slots: None,
                future_leader_slots: 0,
                next_leader_slot: None,
                slots_until_next_leader: None,
                seconds_until_next_leader: None,
                upcoming_leader_slots: vec![],
            }
        };
    metrics.update_snapshot(&Snapshot {
        network: "mainnet-beta".to_string(),
        watched_validators: vec![
            watched_validator(
                "id-1",
                "vote-1",
                ValidatorFundingMetrics {
                    identity_balance_lamports: 3_000_000_000,
                    vote_account_balance_lamports: 2_500_000_000,
                    vote_account_rent_exempt_minimum_lamports: None,
                    vat_reserve_lamports: None,
                    vat_epochs_available: None,
                    vat_next_epoch_funded: None,
                },
            ),
            watched_validator(
                "id-2",
                "vote-2",
                ValidatorFundingMetrics {
                    identity_balance_lamports: 4_000_000_000,
                    vote_account_balance_lamports: 3_500_000_000,
                    vote_account_rent_exempt_minimum_lamports: Some(900_000_000),
                    vat_reserve_lamports: Some(2_600_000_000),
                    vat_epochs_available: Some(3),
                    vat_next_epoch_funded: Some(true),
                },
            ),
        ],
        ..Snapshot::default()
    });

    let rendered = metrics.render()?;
    assert!(rendered.contains("sol_watched_validator_identity_balance_sol{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 3"));
    assert!(rendered.contains("sol_watched_validator_identity_balance_sol{identity=\"id-2\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-2\"} 4"));
    assert!(!rendered
        .contains("sol_watched_validator_vote_account_rent_exempt_minimum_sol{identity=\"id-1\""));
    assert!(rendered.contains("sol_watched_validator_vote_account_rent_exempt_minimum_sol{identity=\"id-2\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-2\"} 0.9"));
    assert!(rendered.contains("sol_watched_validator_vat_reserve_sol{identity=\"id-2\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-2\"} 2.6"));
    assert!(rendered.contains("sol_watched_validator_vat_epochs_available{identity=\"id-2\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-2\"} 3"));
    assert!(rendered.contains("sol_watched_validator_vat_next_epoch_funded{identity=\"id-2\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-2\"} 1"));
    assert!(!rendered.contains("sol_economics_identity_balance_sol{"));
    Ok(())
}

#[test]
fn node_behind_without_distance_omits_catchup_series() -> Result<()> {
    let metrics = Metrics::new()?;
    metrics.update_snapshot(&Snapshot {
        network: "mainnet-beta".to_string(),
        node: Some(NodeMetrics {
            identity: "id-1".to_string(),
            version: "3.1.8".to_string(),
            feature_set: None,
            block_height: 1_233,
            health: NodeHealth::Behind { slots: None },
            voting_status: NodeVotingStatus::Current,
        }),
        ..Snapshot::default()
    });

    let rendered = metrics.render()?;
    assert!(rendered.contains("sol_node_health{identity=\"id-1\",network=\"mainnet-beta\"} 0"));
    assert!(!rendered.contains("sol_node_catchup_distance_slots{"));
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
            rpc: Some(crate::config::RpcTransportConfig::new(
                "https://api.mainnet-beta.solana.com".to_string(),
            )),
            websocket: Some(crate::config::WebsocketTransportConfig::new(
                "wss://api.mainnet-beta.solana.com".to_string(),
            )),
            ..ConnectorConfig::default()
        }],
        modes: modes_for(&["primary"], Some("primary"), false),
        ..Config::default()
    };
    let sources = SourceChain::from_config(
        &config,
        crate::watcher_state::RuntimeTelemetry::unobserved(),
    )?;
    metrics.begin_poll(&config, &sources);
    metrics.update_snapshot(&Snapshot {
        network: network.to_string(),
        epoch: 1,
        slot: 1,
        epoch_slots: 100,
        epoch_progress_pct: 10.0,
        epoch_slots_remaining: 90,
        cluster_performance: Some(ClusterPerformanceMetrics {
            average_slot_time_seconds: Some(0.4),
            transactions_per_second: 25.0,
            vote_transactions_per_second: Some(22.5),
            non_vote_transactions_per_second: Some(2.5),
            slots_per_second: 2.5,
        }),
        estimated_epoch_end_unixtime: Some(1_800_000_000.0),
        vote_credits_sample_epoch: Some(1),
        reward_sample_epoch: Some(1),
        scopes: BTreeMap::new(),
        watched_validators: vec![WatchedValidatorMetrics {
            identity: "old-id".to_string(),
            vote_account: "old-vote".to_string(),
            status: ValidatorStatus::Current,
            gossip: GossipObservation::Present {
                version: Some("old-version".to_string()),
                feature_set: Some(1),
                shred_version: Some(2),
            },
            scopes: vec!["scope:watched".to_string()],
            activated_stake_lamports: 1_000_000_000,
            funding: Some(ValidatorFundingMetrics {
                identity_balance_lamports: 1_000_000_000,
                vote_account_balance_lamports: 1_000_000_000,
                vote_account_rent_exempt_minimum_lamports: Some(500_000_000),
                vat_reserve_lamports: Some(500_000_000),
                vat_epochs_available: Some(0),
                vat_next_epoch_funded: Some(false),
            }),
            latest_epoch_vote_credits: Some(1),
            latest_epoch_reward_lamports: Some(1_000_000_000),
            latest_epoch_rewarded_blocks_produced: 1,
            vote_efficiency_pct: Some(100.0),
            last_vote_slot_lag: 0,
            root_slot_lag: 0,
            commission_pct: 0,
            blocks_produced: 1,
            blocks_missed: 0,
            block_production_rate: 100.0,
            current_epoch_scheduled_leader_slots: Some(1),
            future_leader_slots: 0,
            next_leader_slot: Some(10),
            slots_until_next_leader: Some(9),
            seconds_until_next_leader: Some(3.6),
            upcoming_leader_slots: vec![10],
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
        node: Some(NodeMetrics {
            identity: "old-node-id".to_string(),
            version: "old-version".to_string(),
            feature_set: None,
            block_height: 1,
            health: NodeHealth::Behind { slots: Some(42) },
            voting_status: NodeVotingStatus::Delinquent,
        }),
        economics: Some(EconomicsMetrics {
            identity: "old-id".to_string(),
            vote_account: "old-vote".to_string(),
            identity_balance_lamports: 1_000_000_000,
            vote_account_balance_lamports: 1_000_000_000,
            vote_account_rent_exempt_minimum_lamports: Some(500_000_000),
            vat_reserve_lamports: Some(500_000_000),
            vat_epochs_available: Some(0),
            vat_next_epoch_funded: Some(false),
            jito_mev_reward: Some(JitoMevReward {
                epoch: 0,
                reward_lamports: 1,
                commission_bps: 0,
            }),
            trillium_mev_capture: Some(TrilliumMevCapture {
                epoch: 0,
                captured_sol: Some(1.0),
                validator_sol: Some(0.1),
                stakers_sol: Some(0.8),
                block_engine_sol: Some(0.05),
                tip_router_sol: Some(0.05),
                per_leader_slot_sol: Some(0.01),
                commission_pct: Some(10.0),
                total_jito_tips_sol: Some(1.05),
                total_jito_tip_count: Some(10),
                leader_slots: 100,
                blocks_produced: 99,
                activated_stake_sol: Some(1.0),
                block_rewards: TrilliumBlockRewards {
                    after_burn_sol: Some(1.0),
                    ..TrilliumBlockRewards::default()
                },
                transactions: TrilliumTransactionMetrics {
                    successful: Some(1),
                    ..TrilliumTransactionMetrics::default()
                },
            }),
            recent_block_fee_rewards: Some(RecentBlockFeeRewards {
                latest_slot: 1,
                latest_reward_lamports: 1,
                total_reward_lamports: 1,
                sampled_block_count: 1,
            }),
        }),
    });
    metrics.update_snapshot(&Snapshot {
        network: network.to_string(),
        economics: Some(EconomicsMetrics {
            identity: "partial-id".to_string(),
            vote_account: "partial-vote".to_string(),
            identity_balance_lamports: 0,
            vote_account_balance_lamports: 0,
            vote_account_rent_exempt_minimum_lamports: None,
            vat_reserve_lamports: None,
            vat_epochs_available: None,
            vat_next_epoch_funded: None,
            jito_mev_reward: None,
            trillium_mev_capture: Some(TrilliumMevCapture {
                epoch: 1,
                captured_sol: Some(2.0),
                validator_sol: None,
                stakers_sol: None,
                block_engine_sol: None,
                tip_router_sol: None,
                per_leader_slot_sol: None,
                commission_pct: None,
                total_jito_tips_sol: None,
                total_jito_tip_count: None,
                leader_slots: 10,
                blocks_produced: 9,
                activated_stake_sol: None,
                block_rewards: TrilliumBlockRewards::default(),
                transactions: TrilliumTransactionMetrics::default(),
            }),
            recent_block_fee_rewards: None,
        }),
        ..Snapshot::default()
    });
    let partial = metrics.render()?;
    assert!(partial.contains("sol_economics_trillium_mev_capture_sol{identity=\"partial-id\",network=\"mainnet-beta\",vote_account=\"partial-vote\"} 2"));
    assert!(!partial.contains("sol_economics_trillium_mev_allocation_sol{"));
    assert!(!partial.contains("sol_economics_trillium_block_reward_sol{"));
    assert!(!partial.contains("sol_economics_trillium_per_block_reward_sol{"));
    assert!(!partial.contains("sol_economics_trillium_transactions{"));
    assert!(!partial.contains("sol_economics_trillium_transactions_per_block{"));
    assert!(!partial.contains("sol_economics_trillium_transaction_success_pct{"));
    assert!(!partial.contains("sol_economics_trillium_compute_units_per_block{"));
    assert!(!partial.contains("sol_economics_trillium_activated_stake_sol{"));
    assert!(!partial.contains("sol_economics_trillium_priority_fee_commission_pct{"));

    metrics.update_snapshot(&Snapshot {
        network: network.to_string(),
        epoch: 2,
        slot: 2,
        epoch_slots: 100,
        epoch_progress_pct: 20.0,
        epoch_slots_remaining: 80,
        cluster_performance: None,
        estimated_epoch_end_unixtime: None,
        vote_credits_sample_epoch: None,
        reward_sample_epoch: None,
        scopes: BTreeMap::new(),
        watched_validators: vec![WatchedValidatorMetrics {
            identity: "current-id".to_string(),
            vote_account: "current-vote".to_string(),
            status: ValidatorStatus::Current,
            gossip: GossipObservation::Missing,
            scopes: vec!["scope:watched".to_string()],
            activated_stake_lamports: 1_000_000_000,
            funding: None,
            latest_epoch_vote_credits: None,
            latest_epoch_reward_lamports: None,
            latest_epoch_rewarded_blocks_produced: 3,
            vote_efficiency_pct: None,
            last_vote_slot_lag: 0,
            root_slot_lag: 0,
            commission_pct: 0,
            blocks_produced: 0,
            blocks_missed: 0,
            block_production_rate: 0.0,
            current_epoch_scheduled_leader_slots: Some(1),
            future_leader_slots: 0,
            next_leader_slot: None,
            slots_until_next_leader: None,
            seconds_until_next_leader: None,
            upcoming_leader_slots: vec![],
        }],
        data_sources: vec![],
        mode_statuses: vec![],
        node: None,
        economics: None,
    });

    let rendered = metrics.render()?;
    assert!(!rendered.contains("old-id"));
    assert!(!rendered.contains("old-vote"));
    assert!(!rendered.contains("old-node-id"));
    assert!(!rendered.contains("old-version"));
    assert!(!rendered.contains("sol_node_health{"));
    assert!(!rendered.contains("sol_node_catchup_distance_slots{"));
    assert!(rendered.contains("sol_watched_validator_info{identity=\"current-id\""));
    assert!(rendered.contains("sol_watched_validator_gossip_visible{identity=\"current-id\",network=\"mainnet-beta\",status=\"current\",vote_account=\"current-vote\"} 0"));
    assert!(!rendered.contains("sol_watched_validator_gossip_info{"));
    assert!(!rendered.contains("sol_vote_credits_sample_epoch{"));
    assert!(!rendered.contains("sol_reward_sample_epoch{"));
    assert!(!rendered.contains("sol_watched_validator_vote_credits_latest_epoch{"));
    assert!(!rendered.contains("sol_watched_validator_vote_efficiency_pct{"));
    assert!(rendered.contains("sol_watched_validator_current_epoch_scheduled_leader_slots{identity=\"current-id\",network=\"mainnet-beta\",status=\"current\",vote_account=\"current-vote\"} 1"));
    assert!(!rendered.contains("sol_watched_validator_epoch_reward_sol{identity=\"current-id\""));
    assert!(
        !rendered.contains("sol_watched_validator_reward_per_block_sol{identity=\"current-id\"")
    );
    assert!(rendered.contains("sol_watched_validator_rewarded_blocks_latest_epoch{identity=\"current-id\",network=\"mainnet-beta\",status=\"current\",vote_account=\"current-vote\"} 3"));
    assert!(!rendered
        .contains("sol_watched_validator_scheduled_leader_slots_per_hour{identity=\"current-id\""));
    assert!(
        !rendered.contains("sol_watched_validator_upcoming_leader_slot{identity=\"current-id\"")
    );
    assert!(!rendered.contains("sol_average_slot_time_seconds{"));
    assert!(!rendered.contains("sol_cluster_transactions_per_second{"));
    assert!(!rendered.contains("sol_cluster_vote_transactions_per_second{"));
    assert!(!rendered.contains("sol_cluster_non_vote_transactions_per_second{"));
    assert!(!rendered.contains("sol_cluster_slots_per_second{"));
    assert!(!rendered.contains("sol_estimated_epoch_end_unixtime{"));
    assert!(!rendered.contains("sol_economics_vat_reserve_sol{"));
    assert!(!rendered.contains("sol_watched_validator_vat_reserve_sol{"));
    assert!(!rendered.contains("sol_economics_block_fee_reward_sol{"));
    assert!(!rendered.contains("sol_economics_block_fee_reward_slot{"));
    assert!(!rendered.contains("sol_economics_recent_block_fee_reward_average_sol{"));
    assert!(!rendered.contains("sol_economics_recent_block_fee_reward_sample_count{"));
    assert!(!rendered.contains("sol_economics_trillium_block_reward_sol{"));
    assert!(!rendered.contains("sol_economics_trillium_transactions{"));
    assert!(!rendered.contains("sol_economics_trillium_activated_stake_sol{"));

    Ok(())
}

#[test]
fn watcher_state_metrics_render_bounded_response_counts() -> Result<()> {
    let metrics = Metrics::new()?;
    let failovers = BTreeMap::new();
    let mut state = WatcherStateSnapshot {
        network: Arc::from("devnet"),
        completed_poll_rounds: 12,
        successful_poll_rounds: 5,
        degraded: false,
        ready: true,
        rpc_responses: RpcResponseCounts {
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
    };
    metrics.update_watcher_state(&state, &failovers);

    let rendered = metrics.render()?;
    for sample in [
        "sol_watcher_ready{network=\"devnet\"} 1",
        "sol_watcher_degraded{network=\"devnet\"} 0",
        "sol_watcher_poll_round_count{network=\"devnet\"} 12",
        "sol_watcher_successful_poll_streak{network=\"devnet\"} 5",
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

    state = WatcherStateSnapshot {
        network: Arc::from("mainnet-beta"),
        completed_poll_rounds: 0,
        successful_poll_rounds: 0,
        degraded: false,
        ready: false,
        rpc_responses: RpcResponseCounts::default(),
    };
    metrics.update_watcher_state(&state, &failovers);
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
    metrics.record_block_fee_cache("devnet", "primary", 3, 2, 2);

    let rendered = metrics.render()?;
    for sample in [
        "sol_last_successful_snapshot_timestamp_seconds{network=\"devnet\"} 1800000000.5",
        "sol_snapshot_duration_seconds_count{network=\"devnet\",outcome=\"failure\"} 1",
        "sol_snapshot_duration_seconds_count{network=\"devnet\",outcome=\"success\"} 1",
        "sol_snapshot_failures_total{network=\"devnet\"} 1",
        "sol_rpc_request_duration_seconds_count{connector=\"primary\",method=\"getEpochInfo\",network=\"devnet\"} 1",
        "sol_rpc_request_failures_total{connector=\"primary\",method=\"getEpochInfo\",network=\"devnet\"} 1",
        "sol_rpc_block_fee_cache_accesses_total{connector=\"primary\",network=\"devnet\",outcome=\"hit\"} 3",
        "sol_rpc_block_fee_cache_accesses_total{connector=\"primary\",network=\"devnet\",outcome=\"miss\"} 2",
        "sol_rpc_block_fee_methods_requested_total{connector=\"primary\",network=\"devnet\"} 2",
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
                rpc: Some(crate::config::RpcTransportConfig::new(
                    "https://primary.example".to_string(),
                )),
                ..ConnectorConfig::default()
            },
            ConnectorConfig {
                name: "fallback".to_string(),
                provider: "helius".to_string(),
                features: Vec::new(),
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
                rpc: Some(crate::config::RpcTransportConfig::new(
                    "https://fallback.example".to_string(),
                )),
                ..ConnectorConfig::default()
            },
        ],
        modes: modes_for(&["primary", "fallback"], None, false),
        poll_interval_sec: 47,
        ..Config::default()
    };
    let sources = SourceChain::from_config(
        &config,
        crate::watcher_state::RuntimeTelemetry::unobserved(),
    )?;
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
                node_status: rpc_route(),
            },
        },
        consensus: ConsensusModeConfig {
            commands: ConsensusCommands {
                epoch_info: rpc_route(),
                vote_accounts: rpc_route(),
                cluster_nodes: rpc_route(),
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
                recent_block_fee_rewards: vec![],
                jito_mev_rewards: vec![],
                trillium_mev_capture: vec![],
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
