//! Construction and registration of the watcher's Prometheus collectors.
//!
//! All descriptors are registered in one registry so metric names and label sets
//! remain a single, auditable operational contract.

use super::{register, Metrics, BUILD_REVISION};
use anyhow::Result;
use prometheus::{
    GaugeVec, HistogramOpts, HistogramVec, IntCounterVec, IntGaugeVec, Opts, Registry,
};

const SNAPSHOT_DURATION_BUCKETS_SECONDS: [f64; 10] =
    [0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0];
const RPC_REQUEST_DURATION_BUCKETS_SECONDS: [f64; 11] =
    [0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0];

impl Metrics {
    /// Constructs and registers the complete Prometheus metric contract.
    pub fn new() -> Result<Self> {
        let registry = Registry::new();

        let sol_slot =
            IntGaugeVec::new(Opts::new("sol_slot", "Current Solana slot"), &["network"])?;
        let sol_epoch =
            IntGaugeVec::new(Opts::new("sol_epoch", "Current Solana epoch"), &["network"])?;
        let sol_vote_credits_sample_epoch = IntGaugeVec::new(
            Opts::new(
                "sol_vote_credits_sample_epoch",
                "Epoch used for sol_vote_credits_latest_epoch and sol_vote_efficiency_pct",
            ),
            &["network"],
        )?;
        let sol_reward_sample_epoch = IntGaugeVec::new(
            Opts::new(
                "sol_reward_sample_epoch",
                "Epoch used for latest epoch reward metrics",
            ),
            &["network"],
        )?;
        let sol_epoch_progress_pct = GaugeVec::new(
            Opts::new(
                "sol_epoch_progress_pct",
                "Completed percentage of the current epoch",
            ),
            &["network"],
        )?;
        let sol_epoch_slots_remaining = IntGaugeVec::new(
            Opts::new(
                "sol_epoch_slots_remaining",
                "Slots remaining in the current epoch",
            ),
            &["network"],
        )?;
        let sol_average_slot_time_seconds = GaugeVec::new(
            Opts::new(
                "sol_average_slot_time_seconds",
                "Average slot time from the latest Solana performance sample",
            ),
            &["network"],
        )?;
        let sol_cluster_transactions_per_second = GaugeVec::new(
            Opts::new(
                "sol_cluster_transactions_per_second",
                "All transactions processed per second in the latest Solana performance sample, including votes",
            ),
            &["network"],
        )?;
        let sol_cluster_vote_transactions_per_second = GaugeVec::new(
            Opts::new(
                "sol_cluster_vote_transactions_per_second",
                "Vote transactions processed per second in the latest Solana performance sample",
            ),
            &["network"],
        )?;
        let sol_cluster_non_vote_transactions_per_second = GaugeVec::new(
            Opts::new(
                "sol_cluster_non_vote_transactions_per_second",
                "Non-vote transactions processed per second in the latest Solana performance sample",
            ),
            &["network"],
        )?;
        let sol_cluster_slots_per_second = GaugeVec::new(
            Opts::new(
                "sol_cluster_slots_per_second",
                "Slots completed per second in the latest Solana performance sample",
            ),
            &["network"],
        )?;
        let sol_estimated_epoch_end_unixtime = GaugeVec::new(
            Opts::new(
                "sol_estimated_epoch_end_unixtime",
                "Estimated Unix time when the current epoch will end",
            ),
            &["network"],
        )?;
        let sol_watcher_build_info = IntGaugeVec::new(
            Opts::new(
                "sol_watcher_build_info",
                "Watcher binary version and source revision",
            ),
            &["version", "revision"],
        )?;
        sol_watcher_build_info
            .with_label_values(&[env!("CARGO_PKG_VERSION"), BUILD_REVISION])
            .set(1);
        let sol_config_info = IntGaugeVec::new(
            Opts::new(
                "sol_config_info",
                "SHA-256 fingerprint of the fully expanded active profile",
            ),
            &["network", "fingerprint"],
        )?;
        let sol_config_reload_total = IntCounterVec::new(
            Opts::new(
                "sol_config_reload_total",
                "Changed profile reload attempts by outcome",
            ),
            &["network", "outcome"],
        )?;
        let sol_watcher_up = IntGaugeVec::new(
            Opts::new("sol_watcher_up", "Whether the watcher process is running"),
            &["network"],
        )?;
        let sol_watcher_ready = IntGaugeVec::new(
            Opts::new(
                "sol_watcher_ready",
                "Whether five consecutive polling rounds completed without failed RPC attempts or non-2xx responses",
            ),
            &["network"],
        )?;
        let sol_watcher_poll_round_count = IntGaugeVec::new(
            Opts::new(
                "sol_watcher_poll_round_count",
                "Completed polling rounds for the current accepted configuration",
            ),
            &["network"],
        )?;
        let sol_watcher_clean_poll_streak = IntGaugeVec::new(
            Opts::new(
                "sol_watcher_clean_poll_streak",
                "Consecutive clean polling rounds toward watcher readiness",
            ),
            &["network"],
        )?;
        let sol_watcher_poll_interval_seconds = IntGaugeVec::new(
            Opts::new(
                "sol_watcher_poll_interval_seconds",
                "Configured interval between completed polling rounds",
            ),
            &["network"],
        )?;
        let sol_last_successful_snapshot_timestamp_seconds = GaugeVec::new(
            Opts::new(
                "sol_last_successful_snapshot_timestamp_seconds",
                "Unix timestamp when the latest complete snapshot was published",
            ),
            &["network"],
        )?;
        let sol_snapshot_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "sol_snapshot_duration_seconds",
                "Time spent producing one complete or failed snapshot",
            )
            .buckets(SNAPSHOT_DURATION_BUCKETS_SECONDS.to_vec()),
            &["network", "outcome"],
        )?;
        let sol_snapshot_failures_total = IntCounterVec::new(
            Opts::new(
                "sol_snapshot_failures_total",
                "Polling rounds that failed before publishing a complete snapshot for the current accepted configuration",
            ),
            &["network"],
        )?;
        let sol_rpc_http_response_class_count = IntGaugeVec::new(
            Opts::new(
                "sol_rpc_http_response_class_count",
                "Cumulative RPC HTTP responses observed by status class since process start",
            ),
            &["network", "status_class"],
        )?;
        let sol_rpc_http_response_status_count = IntGaugeVec::new(
            Opts::new(
                "sol_rpc_http_response_status_count",
                "Cumulative RPC HTTP responses for readiness-critical status codes since process start",
            ),
            &["network", "status_code"],
        )?;
        let sol_rpc_request_failure_count = IntGaugeVec::new(
            Opts::new(
                "sol_rpc_request_failure_count",
                "Cumulative failed RPC attempts since process start",
            ),
            &["network"],
        )?;
        let sol_rpc_request_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "sol_rpc_request_duration_seconds",
                "End-to-end duration of one Solana JSON-RPC request",
            )
            .buckets(RPC_REQUEST_DURATION_BUCKETS_SECONDS.to_vec()),
            &["network", "connector", "method"],
        )?;
        let sol_rpc_request_failures_total = IntCounterVec::new(
            Opts::new(
                "sol_rpc_request_failures_total",
                "Failed Solana JSON-RPC attempts by connector and method for the current accepted configuration",
            ),
            &["network", "connector", "method"],
        )?;
        let sol_rpc_block_fee_cache_accesses_total = IntCounterVec::new(
            Opts::new(
                "sol_rpc_block_fee_cache_accesses_total",
                "Finalized block-fee cache lookups by connector and outcome",
            ),
            &["network", "connector", "outcome"],
        )?;
        let sol_rpc_block_fee_methods_requested_total = IntCounterVec::new(
            Opts::new(
                "sol_rpc_block_fee_methods_requested_total",
                "getBlock methods sent for block-fee samples",
            ),
            &["network", "connector"],
        )?;
        let sol_rpc_up = IntGaugeVec::new(
            Opts::new(
                "sol_rpc_up",
                "Whether the last Solana RPC collection succeeded",
            ),
            &["network"],
        )?;
        let sol_ws_up = IntGaugeVec::new(
            Opts::new(
                "sol_ws_up",
                "Whether the configured websocket transport probe succeeded",
            ),
            &["network"],
        )?;
        let sol_ws_probe_failures_total = IntCounterVec::new(
            Opts::new(
                "sol_ws_probe_failures_total",
                "Failed WebSocket reachability probes by connector for the current accepted configuration",
            ),
            &["network", "connector"],
        )?;
        let sol_watcher_config_info = IntGaugeVec::new(
            Opts::new("sol_watcher_config_info", "Watcher transport configuration"),
            &["network", "transport_mode", "ws_configured"],
        )?;
        let sol_connector_info = IntGaugeVec::new(
            Opts::new(
                "sol_connector_info",
                "Configured connector inventory, provider, standard Solana RPC compatibility, and available transports",
            ),
            &[
                "network",
                "connector",
                "provider",
                "declaration_order",
                "solana_rpc_compatibility",
                "has_rpc",
                "has_ws",
                "has_provider_api",
            ],
        )?;
        let sol_connector_feature_info = IntGaugeVec::new(
            Opts::new(
                "sol_connector_feature_info",
                "Provider features declared for each configured connector",
            ),
            &["network", "connector", "provider", "feature"],
        )?;
        let sol_mode_enabled = IntGaugeVec::new(
            Opts::new("sol_mode_enabled", "Whether a collection mode is enabled"),
            &["network", "mode"],
        )?;
        let sol_mode_up = IntGaugeVec::new(
            Opts::new(
                "sol_mode_up",
                "Whether the latest execution of an enabled collection mode succeeded",
            ),
            &["network", "mode"],
        )?;
        let sol_mode_command_route_info = IntGaugeVec::new(
            Opts::new(
                "sol_mode_command_route_info",
                "Configured connector fallback order, transport, declared and required standard Solana RPC compatibility, and optional provider feature for each mode command",
            ),
            &[
                "network",
                "mode",
                "command",
                "connector",
                "provider",
                "transport",
                "solana_rpc_compatibility",
                "required_solana_rpc_compatibility",
                "feature",
                "precedence",
                "available",
            ],
        )?;
        let sol_mode_command_source_info = IntGaugeVec::new(
            Opts::new(
                "sol_mode_command_source_info",
                "Connector, provider, transport, declared and required standard Solana RPC compatibility, and optional feature that supplied each mode command's latest result",
            ),
            &[
                "network",
                "mode",
                "command",
                "connector",
                "provider",
                "transport",
                "solana_rpc_compatibility",
                "required_solana_rpc_compatibility",
                "feature",
                "precedence",
            ],
        )?;
        let sol_mode_command_fallback_active = IntGaugeVec::new(
            Opts::new(
                "sol_mode_command_fallback_active",
                "Whether a mode command's latest value came from a fallback connector",
            ),
            &["network", "mode", "command", "connector"],
        )?;
        let sol_node_info = IntGaugeVec::new(
            Opts::new(
                "sol_node_info",
                "Node-local identity, validator client version, feature set, and cluster-observed vote-account state",
            ),
            &[
                "network",
                "identity",
                "version",
                "feature_set",
                "voting_status",
            ],
        )?;
        let sol_node_health = IntGaugeVec::new(
            Opts::new(
                "sol_node_health",
                "Whether the node-local RPC endpoint reports healthy relative to the cluster tip",
            ),
            &["network", "identity"],
        )?;
        let sol_node_catchup_distance_slots = IntGaugeVec::new(
            Opts::new(
                "sol_node_catchup_distance_slots",
                "Slots the node-local RPC endpoint reports behind the cluster tip",
            ),
            &["network", "identity"],
        )?;
        let sol_node_block_height = IntGaugeVec::new(
            Opts::new(
                "sol_node_block_height",
                "Block height reported by the node-local RPC endpoint",
            ),
            &["network", "identity"],
        )?;

        let sol_validator_status_count = GaugeVec::new(
            Opts::new(
                "sol_validator_status_count",
                "Validator count split by scope and status",
            ),
            &["scope", "status", "network"],
        )?;
        let sol_validator_status_stake_sol = GaugeVec::new(
            Opts::new(
                "sol_validator_status_stake_sol",
                "Activated stake in SOL split by scope and status",
            ),
            &["scope", "status", "network"],
        )?;
        let sol_activated_stake_sol = GaugeVec::new(
            Opts::new(
                "sol_activated_stake_sol",
                "Total activated stake in SOL for a scope",
            ),
            &["scope", "network"],
        )?;
        let sol_delinquent_validator_count = GaugeVec::new(
            Opts::new(
                "sol_delinquent_validator_count",
                "Delinquent validator count for a scope",
            ),
            &["scope", "network"],
        )?;
        let sol_delinquent_stake_sol = GaugeVec::new(
            Opts::new(
                "sol_delinquent_stake_sol",
                "Delinquent activated stake in SOL for a scope",
            ),
            &["scope", "network"],
        )?;
        let sol_vote_credits_latest_epoch = GaugeVec::new(
            Opts::new(
                "sol_vote_credits_latest_epoch",
                "Sum of latest available epoch vote credits for a scope",
            ),
            &["scope", "network"],
        )?;
        let sol_epoch_reward_sol = GaugeVec::new(
            Opts::new(
                "sol_epoch_reward_sol",
                "Latest sampled epoch reward in SOL for a scope",
            ),
            &["scope", "network"],
        )?;
        let sol_rewarded_blocks_latest_epoch = GaugeVec::new(
            Opts::new(
                "sol_rewarded_blocks_latest_epoch",
                "Produced blocks in the sampled reward epoch for a scope",
            ),
            &["scope", "network"],
        )?;
        let sol_reward_per_block_sol = GaugeVec::new(
            Opts::new(
                "sol_reward_per_block_sol",
                "Latest sampled epoch reward divided by produced blocks for a scope",
            ),
            &["scope", "network"],
        )?;
        let sol_vote_efficiency_pct = GaugeVec::new(
            Opts::new(
                "sol_vote_efficiency_pct",
                "Stake-weighted vote efficiency versus the best validator in the sampled epoch",
            ),
            &["scope", "network"],
        )?;
        let sol_last_vote_slot_lag = GaugeVec::new(
            Opts::new(
                "sol_last_vote_slot_lag",
                "Stake-weighted average lag between current slot and validator last_vote",
            ),
            &["scope", "network"],
        )?;
        let sol_root_slot_lag = GaugeVec::new(
            Opts::new(
                "sol_root_slot_lag",
                "Stake-weighted average lag between current slot and validator root_slot",
            ),
            &["scope", "network"],
        )?;
        let sol_average_commission_pct = GaugeVec::new(
            Opts::new(
                "sol_average_commission_pct",
                "Stake-weighted average commission percentage for a scope",
            ),
            &["scope", "network"],
        )?;
        let sol_blocks_produced = GaugeVec::new(
            Opts::new(
                "sol_blocks_produced",
                "Blocks produced in the current epoch range for a scope",
            ),
            &["scope", "network"],
        )?;
        let sol_blocks_missed = GaugeVec::new(
            Opts::new(
                "sol_blocks_missed",
                "Leader slots without a produced block in the current epoch range for a scope",
            ),
            &["scope", "network"],
        )?;
        let sol_block_production_rate = GaugeVec::new(
            Opts::new(
                "sol_block_production_rate",
                "Block production rate in percent for the current epoch range",
            ),
            &["scope", "network"],
        )?;
        let sol_future_leader_slots = GaugeVec::new(
            Opts::new(
                "sol_future_leader_slots",
                "Scheduled future leader slots in the current and next epoch",
            ),
            &["scope", "network"],
        )?;
        let economics_labels = &["network", "identity", "vote_account"];
        let sol_economics_identity_balance_sol = GaugeVec::new(
            Opts::new(
                "sol_economics_identity_balance_sol",
                "Identity account balance in SOL for the configured economics validator",
            ),
            economics_labels,
        )?;
        let sol_economics_vote_account_balance_sol = GaugeVec::new(
            Opts::new(
                "sol_economics_vote_account_balance_sol",
                "Vote account balance in SOL for the configured economics validator",
            ),
            economics_labels,
        )?;
        let sol_economics_vote_account_rent_exempt_minimum_sol = GaugeVec::new(
            Opts::new(
                "sol_economics_vote_account_rent_exempt_minimum_sol",
                "Rent-exempt minimum in SOL for the configured economics vote account",
            ),
            economics_labels,
        )?;
        let sol_economics_vat_reserve_sol = GaugeVec::new(
            Opts::new(
                "sol_economics_vat_reserve_sol",
                "Vote account balance above rent exemption available for future VAT charges",
            ),
            economics_labels,
        )?;
        let sol_economics_vat_epochs_available = IntGaugeVec::new(
            Opts::new(
                "sol_economics_vat_epochs_available",
                "Complete future 0.8 SOL VAT charges funded above rent exemption",
            ),
            economics_labels,
        )?;
        let sol_economics_vat_next_epoch_funded = IntGaugeVec::new(
            Opts::new(
                "sol_economics_vat_next_epoch_funded",
                "Whether the vote account can fund the next 0.8 SOL VAT charge above rent exemption",
            ),
            economics_labels,
        )?;
        let sol_economics_block_fee_reward_sol = GaugeVec::new(
            Opts::new(
                "sol_economics_block_fee_reward_sol",
                "Validator fee reward in SOL for the latest sampled confirmed block",
            ),
            economics_labels,
        )?;
        let sol_economics_block_fee_reward_slot = IntGaugeVec::new(
            Opts::new(
                "sol_economics_block_fee_reward_slot",
                "Absolute slot of the latest sampled confirmed validator fee reward",
            ),
            economics_labels,
        )?;
        let sol_economics_recent_block_fee_reward_average_sol = GaugeVec::new(
            Opts::new(
                "sol_economics_recent_block_fee_reward_average_sol",
                "Average validator fee reward in SOL across recent sampled produced blocks",
            ),
            economics_labels,
        )?;
        let sol_economics_recent_block_fee_reward_sample_count = IntGaugeVec::new(
            Opts::new(
                "sol_economics_recent_block_fee_reward_sample_count",
                "Produced blocks represented by the recent validator fee reward sample",
            ),
            economics_labels,
        )?;
        let sol_economics_jito_mev_reward_sol = GaugeVec::new(
            Opts::new(
                "sol_economics_jito_mev_reward_sol",
                "Post-commission Jito MEV reward in SOL paid to the economics validator and its stakers",
            ),
            economics_labels,
        )?;
        let sol_economics_jito_mev_commission_pct = GaugeVec::new(
            Opts::new(
                "sol_economics_jito_mev_commission_pct",
                "Jito MEV commission percentage for the configured economics validator",
            ),
            economics_labels,
        )?;
        let sol_economics_jito_mev_reward_epoch = IntGaugeVec::new(
            Opts::new(
                "sol_economics_jito_mev_reward_epoch",
                "Settled epoch represented by the configured economics validator Jito MEV reward",
            ),
            economics_labels,
        )?;
        let sol_watched_validator_info = IntGaugeVec::new(
            Opts::new(
                "sol_watched_validator_info",
                "Identity and status for each watched validator",
            ),
            &["network", "identity", "vote_account", "status"],
        )?;
        let sol_watched_validator_scope_info = IntGaugeVec::new(
            Opts::new(
                "sol_watched_validator_scope_info",
                "Scope membership for each watched validator",
            ),
            &["network", "identity", "vote_account", "scope"],
        )?;
        let sol_watched_validator_activated_stake_sol = GaugeVec::new(
            Opts::new(
                "sol_watched_validator_activated_stake_sol",
                "Activated stake in SOL for each watched validator",
            ),
            &["network", "identity", "vote_account", "status"],
        )?;
        let sol_watched_validator_vote_credits_latest_epoch = GaugeVec::new(
            Opts::new(
                "sol_watched_validator_vote_credits_latest_epoch",
                "Latest sampled epoch vote credits for each watched validator",
            ),
            &["network", "identity", "vote_account", "status"],
        )?;
        let sol_watched_validator_epoch_reward_sol = GaugeVec::new(
            Opts::new(
                "sol_watched_validator_epoch_reward_sol",
                "Latest sampled epoch reward in SOL for each watched validator",
            ),
            &["network", "identity", "vote_account", "status"],
        )?;
        let sol_watched_validator_rewarded_blocks_latest_epoch = GaugeVec::new(
            Opts::new(
                "sol_watched_validator_rewarded_blocks_latest_epoch",
                "Produced blocks in the sampled reward epoch for each watched validator",
            ),
            &["network", "identity", "vote_account", "status"],
        )?;
        let sol_watched_validator_reward_per_block_sol = GaugeVec::new(
            Opts::new(
                "sol_watched_validator_reward_per_block_sol",
                "Latest sampled epoch reward divided by produced blocks for each watched validator",
            ),
            &["network", "identity", "vote_account", "status"],
        )?;
        let sol_watched_validator_vote_efficiency_pct = GaugeVec::new(
            Opts::new(
                "sol_watched_validator_vote_efficiency_pct",
                "Vote efficiency versus the best sampled validator for each watched validator",
            ),
            &["network", "identity", "vote_account", "status"],
        )?;
        let sol_watched_validator_last_vote_slot_lag = GaugeVec::new(
            Opts::new(
                "sol_watched_validator_last_vote_slot_lag",
                "Current slot minus last_vote for each watched validator",
            ),
            &["network", "identity", "vote_account", "status"],
        )?;
        let sol_watched_validator_root_slot_lag = GaugeVec::new(
            Opts::new(
                "sol_watched_validator_root_slot_lag",
                "Current slot minus root_slot for each watched validator",
            ),
            &["network", "identity", "vote_account", "status"],
        )?;
        let sol_watched_validator_commission_pct = GaugeVec::new(
            Opts::new(
                "sol_watched_validator_commission_pct",
                "Commission percentage for each watched validator",
            ),
            &["network", "identity", "vote_account", "status"],
        )?;
        let sol_watched_validator_blocks_produced = GaugeVec::new(
            Opts::new(
                "sol_watched_validator_blocks_produced",
                "Blocks produced in the sampled epoch window for each watched validator",
            ),
            &["network", "identity", "vote_account", "status"],
        )?;
        let sol_watched_validator_blocks_missed = GaugeVec::new(
            Opts::new(
                "sol_watched_validator_blocks_missed",
                "Missed leader slots in the sampled epoch window for each watched validator",
            ),
            &["network", "identity", "vote_account", "status"],
        )?;
        let sol_watched_validator_block_production_rate = GaugeVec::new(
            Opts::new(
                "sol_watched_validator_block_production_rate",
                "Block production rate in percent for each watched validator",
            ),
            &["network", "identity", "vote_account", "status"],
        )?;
        let sol_watched_validator_future_leader_slots = GaugeVec::new(
            Opts::new(
                "sol_watched_validator_future_leader_slots",
                "Scheduled future leader slots for each watched validator",
            ),
            &["network", "identity", "vote_account", "status"],
        )?;
        let sol_watched_validator_current_epoch_scheduled_leader_slots = IntGaugeVec::new(
            Opts::new(
                "sol_watched_validator_current_epoch_scheduled_leader_slots",
                "Total scheduled leader slots in the current epoch for each watched validator",
            ),
            &["network", "identity", "vote_account", "status"],
        )?;
        let sol_watched_validator_scheduled_leader_slots_per_hour = GaugeVec::new(
            Opts::new(
                "sol_watched_validator_scheduled_leader_slots_per_hour",
                "Average scheduled block-production opportunities per hour in the current epoch",
            ),
            &["network", "identity", "vote_account", "status"],
        )?;
        let sol_watched_validator_upcoming_leader_slot = IntGaugeVec::new(
            Opts::new(
                "sol_watched_validator_upcoming_leader_slot",
                "Absolute slot for each bounded upcoming watched-validator leader assignment",
            ),
            &["network", "identity", "vote_account", "status", "position"],
        )?;
        let sol_watched_validator_next_leader_slot = IntGaugeVec::new(
            Opts::new(
                "sol_watched_validator_next_leader_slot",
                "Next scheduled leader slot for each watched validator",
            ),
            &["network", "identity", "vote_account", "status"],
        )?;
        let sol_watched_validator_slots_until_next_leader = IntGaugeVec::new(
            Opts::new(
                "sol_watched_validator_slots_until_next_leader",
                "Slots until the next scheduled leader slot for each watched validator",
            ),
            &["network", "identity", "vote_account", "status"],
        )?;
        let sol_watched_validator_seconds_until_next_leader = GaugeVec::new(
            Opts::new(
                "sol_watched_validator_seconds_until_next_leader",
                "Estimated seconds until the next scheduled leader slot for each watched validator",
            ),
            &["network", "identity", "vote_account", "status"],
        )?;

        register(&registry, &sol_slot)?;
        register(&registry, &sol_epoch)?;
        register(&registry, &sol_vote_credits_sample_epoch)?;
        register(&registry, &sol_reward_sample_epoch)?;
        register(&registry, &sol_epoch_progress_pct)?;
        register(&registry, &sol_epoch_slots_remaining)?;
        register(&registry, &sol_average_slot_time_seconds)?;
        register(&registry, &sol_cluster_transactions_per_second)?;
        register(&registry, &sol_cluster_vote_transactions_per_second)?;
        register(&registry, &sol_cluster_non_vote_transactions_per_second)?;
        register(&registry, &sol_cluster_slots_per_second)?;
        register(&registry, &sol_estimated_epoch_end_unixtime)?;
        register(&registry, &sol_watcher_build_info)?;
        register(&registry, &sol_config_info)?;
        register(&registry, &sol_config_reload_total)?;
        register(&registry, &sol_watcher_up)?;
        register(&registry, &sol_rpc_up)?;
        register(&registry, &sol_ws_up)?;
        register(&registry, &sol_ws_probe_failures_total)?;
        register(&registry, &sol_watcher_config_info)?;
        register(&registry, &sol_connector_info)?;
        register(&registry, &sol_connector_feature_info)?;
        register(&registry, &sol_watcher_ready)?;
        register(&registry, &sol_watcher_poll_round_count)?;
        register(&registry, &sol_watcher_clean_poll_streak)?;
        register(&registry, &sol_watcher_poll_interval_seconds)?;
        register(&registry, &sol_last_successful_snapshot_timestamp_seconds)?;
        register(&registry, &sol_snapshot_duration_seconds)?;
        register(&registry, &sol_snapshot_failures_total)?;
        register(&registry, &sol_rpc_http_response_class_count)?;
        register(&registry, &sol_rpc_http_response_status_count)?;
        register(&registry, &sol_rpc_request_failure_count)?;
        register(&registry, &sol_rpc_request_duration_seconds)?;
        register(&registry, &sol_rpc_request_failures_total)?;
        register(&registry, &sol_rpc_block_fee_cache_accesses_total)?;
        register(&registry, &sol_rpc_block_fee_methods_requested_total)?;
        register(&registry, &sol_mode_enabled)?;
        register(&registry, &sol_mode_up)?;
        register(&registry, &sol_mode_command_route_info)?;
        register(&registry, &sol_mode_command_source_info)?;
        register(&registry, &sol_mode_command_fallback_active)?;
        register(&registry, &sol_node_info)?;
        register(&registry, &sol_node_health)?;
        register(&registry, &sol_node_catchup_distance_slots)?;
        register(&registry, &sol_node_block_height)?;
        register(&registry, &sol_validator_status_count)?;
        register(&registry, &sol_validator_status_stake_sol)?;
        register(&registry, &sol_activated_stake_sol)?;
        register(&registry, &sol_delinquent_validator_count)?;
        register(&registry, &sol_delinquent_stake_sol)?;
        register(&registry, &sol_vote_credits_latest_epoch)?;
        register(&registry, &sol_epoch_reward_sol)?;
        register(&registry, &sol_rewarded_blocks_latest_epoch)?;
        register(&registry, &sol_reward_per_block_sol)?;
        register(&registry, &sol_vote_efficiency_pct)?;
        register(&registry, &sol_last_vote_slot_lag)?;
        register(&registry, &sol_root_slot_lag)?;
        register(&registry, &sol_average_commission_pct)?;
        register(&registry, &sol_blocks_produced)?;
        register(&registry, &sol_blocks_missed)?;
        register(&registry, &sol_block_production_rate)?;
        register(&registry, &sol_future_leader_slots)?;
        register(&registry, &sol_economics_identity_balance_sol)?;
        register(&registry, &sol_economics_vote_account_balance_sol)?;
        register(
            &registry,
            &sol_economics_vote_account_rent_exempt_minimum_sol,
        )?;
        register(&registry, &sol_economics_vat_reserve_sol)?;
        register(&registry, &sol_economics_vat_epochs_available)?;
        register(&registry, &sol_economics_vat_next_epoch_funded)?;
        register(&registry, &sol_economics_block_fee_reward_sol)?;
        register(&registry, &sol_economics_block_fee_reward_slot)?;
        register(
            &registry,
            &sol_economics_recent_block_fee_reward_average_sol,
        )?;
        register(
            &registry,
            &sol_economics_recent_block_fee_reward_sample_count,
        )?;
        register(&registry, &sol_economics_jito_mev_reward_sol)?;
        register(&registry, &sol_economics_jito_mev_commission_pct)?;
        register(&registry, &sol_economics_jito_mev_reward_epoch)?;
        register(&registry, &sol_watched_validator_info)?;
        register(&registry, &sol_watched_validator_scope_info)?;
        register(&registry, &sol_watched_validator_activated_stake_sol)?;
        register(&registry, &sol_watched_validator_vote_credits_latest_epoch)?;
        register(&registry, &sol_watched_validator_epoch_reward_sol)?;
        register(
            &registry,
            &sol_watched_validator_rewarded_blocks_latest_epoch,
        )?;
        register(&registry, &sol_watched_validator_reward_per_block_sol)?;
        register(&registry, &sol_watched_validator_vote_efficiency_pct)?;
        register(&registry, &sol_watched_validator_last_vote_slot_lag)?;
        register(&registry, &sol_watched_validator_root_slot_lag)?;
        register(&registry, &sol_watched_validator_commission_pct)?;
        register(&registry, &sol_watched_validator_blocks_produced)?;
        register(&registry, &sol_watched_validator_blocks_missed)?;
        register(&registry, &sol_watched_validator_block_production_rate)?;
        register(&registry, &sol_watched_validator_future_leader_slots)?;
        register(
            &registry,
            &sol_watched_validator_current_epoch_scheduled_leader_slots,
        )?;
        register(
            &registry,
            &sol_watched_validator_scheduled_leader_slots_per_hour,
        )?;
        register(&registry, &sol_watched_validator_upcoming_leader_slot)?;
        register(&registry, &sol_watched_validator_next_leader_slot)?;
        register(&registry, &sol_watched_validator_slots_until_next_leader)?;
        register(&registry, &sol_watched_validator_seconds_until_next_leader)?;

        Ok(Self {
            registry,
            sol_slot,
            sol_epoch,
            sol_vote_credits_sample_epoch,
            sol_reward_sample_epoch,
            sol_epoch_progress_pct,
            sol_epoch_slots_remaining,
            sol_average_slot_time_seconds,
            sol_cluster_transactions_per_second,
            sol_cluster_vote_transactions_per_second,
            sol_cluster_non_vote_transactions_per_second,
            sol_cluster_slots_per_second,
            sol_estimated_epoch_end_unixtime,
            sol_config_info,
            sol_config_reload_total,
            sol_watcher_up,
            sol_rpc_up,
            sol_ws_up,
            sol_ws_probe_failures_total,
            sol_watcher_config_info,
            sol_connector_info,
            sol_connector_feature_info,
            sol_mode_enabled,
            sol_mode_up,
            sol_watcher_ready,
            sol_watcher_poll_round_count,
            sol_watcher_clean_poll_streak,
            sol_watcher_poll_interval_seconds,
            sol_last_successful_snapshot_timestamp_seconds,
            sol_snapshot_duration_seconds,
            sol_snapshot_failures_total,
            sol_rpc_http_response_class_count,
            sol_rpc_http_response_status_count,
            sol_rpc_request_failure_count,
            sol_rpc_request_duration_seconds,
            sol_rpc_request_failures_total,
            sol_rpc_block_fee_cache_accesses_total,
            sol_rpc_block_fee_methods_requested_total,
            sol_mode_command_route_info,
            sol_mode_command_source_info,
            sol_mode_command_fallback_active,
            sol_node_info,
            sol_node_health,
            sol_node_catchup_distance_slots,
            sol_node_block_height,
            sol_validator_status_count,
            sol_validator_status_stake_sol,
            sol_activated_stake_sol,
            sol_delinquent_validator_count,
            sol_delinquent_stake_sol,
            sol_vote_credits_latest_epoch,
            sol_epoch_reward_sol,
            sol_rewarded_blocks_latest_epoch,
            sol_reward_per_block_sol,
            sol_vote_efficiency_pct,
            sol_last_vote_slot_lag,
            sol_root_slot_lag,
            sol_average_commission_pct,
            sol_blocks_produced,
            sol_blocks_missed,
            sol_block_production_rate,
            sol_future_leader_slots,
            sol_economics_identity_balance_sol,
            sol_economics_vote_account_balance_sol,
            sol_economics_vote_account_rent_exempt_minimum_sol,
            sol_economics_vat_reserve_sol,
            sol_economics_vat_epochs_available,
            sol_economics_vat_next_epoch_funded,
            sol_economics_block_fee_reward_sol,
            sol_economics_block_fee_reward_slot,
            sol_economics_recent_block_fee_reward_average_sol,
            sol_economics_recent_block_fee_reward_sample_count,
            sol_economics_jito_mev_reward_sol,
            sol_economics_jito_mev_commission_pct,
            sol_economics_jito_mev_reward_epoch,
            sol_watched_validator_info,
            sol_watched_validator_scope_info,
            sol_watched_validator_activated_stake_sol,
            sol_watched_validator_vote_credits_latest_epoch,
            sol_watched_validator_epoch_reward_sol,
            sol_watched_validator_rewarded_blocks_latest_epoch,
            sol_watched_validator_reward_per_block_sol,
            sol_watched_validator_vote_efficiency_pct,
            sol_watched_validator_last_vote_slot_lag,
            sol_watched_validator_root_slot_lag,
            sol_watched_validator_commission_pct,
            sol_watched_validator_blocks_produced,
            sol_watched_validator_blocks_missed,
            sol_watched_validator_block_production_rate,
            sol_watched_validator_future_leader_slots,
            sol_watched_validator_current_epoch_scheduled_leader_slots,
            sol_watched_validator_scheduled_leader_slots_per_hour,
            sol_watched_validator_upcoming_leader_slot,
            sol_watched_validator_next_leader_slot,
            sol_watched_validator_slots_until_next_leader,
            sol_watched_validator_seconds_until_next_leader,
        })
    }
}
