use crate::{
    collector::{Snapshot, WatchedValidatorMetrics},
    config::{Config, ConnectorConfig},
    mode::{ModeId, ProviderFeature, SolanaRpcCompatibility},
    source::SourceChain,
};
use anyhow::{Context, Result};
use prometheus::{Encoder, GaugeVec, IntGaugeVec, Opts, Registry, TextEncoder};

const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

#[derive(Clone)]
pub struct Metrics {
    registry: Registry,
    sol_slot: IntGaugeVec,
    sol_epoch: IntGaugeVec,
    sol_vote_credits_sample_epoch: IntGaugeVec,
    sol_reward_sample_epoch: IntGaugeVec,
    sol_epoch_progress_pct: GaugeVec,
    sol_epoch_slots_remaining: IntGaugeVec,
    sol_average_slot_time_seconds: GaugeVec,
    sol_estimated_epoch_end_unixtime: GaugeVec,
    sol_watcher_up: IntGaugeVec,
    sol_rpc_up: IntGaugeVec,
    sol_ws_up: IntGaugeVec,
    sol_watcher_config_info: IntGaugeVec,
    sol_connector_info: IntGaugeVec,
    sol_connector_feature_info: IntGaugeVec,
    sol_mode_enabled: IntGaugeVec,
    sol_mode_up: IntGaugeVec,
    sol_mode_command_route_info: IntGaugeVec,
    sol_mode_command_source_info: IntGaugeVec,
    sol_validator_status_count: GaugeVec,
    sol_validator_status_stake_sol: GaugeVec,
    sol_activated_stake_sol: GaugeVec,
    sol_delinquent_validator_count: GaugeVec,
    sol_delinquent_stake_sol: GaugeVec,
    sol_vote_credits_latest_epoch: GaugeVec,
    sol_epoch_reward_sol: GaugeVec,
    sol_rewarded_blocks_latest_epoch: GaugeVec,
    sol_reward_per_block_sol: GaugeVec,
    sol_vote_efficiency_pct: GaugeVec,
    sol_last_vote_slot_lag: GaugeVec,
    sol_root_slot_lag: GaugeVec,
    sol_average_commission_pct: GaugeVec,
    sol_blocks_produced: GaugeVec,
    sol_blocks_missed: GaugeVec,
    sol_block_production_rate: GaugeVec,
    sol_future_leader_slots: GaugeVec,
    sol_economics_identity_balance_sol: GaugeVec,
    sol_economics_vote_account_balance_sol: GaugeVec,
    sol_economics_vote_account_rent_exempt_minimum_sol: GaugeVec,
    sol_economics_vat_reserve_sol: GaugeVec,
    sol_economics_vat_epochs_available: IntGaugeVec,
    sol_economics_vat_next_epoch_funded: IntGaugeVec,
    sol_watched_validator_info: IntGaugeVec,
    sol_watched_validator_scope_info: IntGaugeVec,
    sol_watched_validator_activated_stake_sol: GaugeVec,
    sol_watched_validator_vote_credits_latest_epoch: GaugeVec,
    sol_watched_validator_epoch_reward_sol: GaugeVec,
    sol_watched_validator_rewarded_blocks_latest_epoch: GaugeVec,
    sol_watched_validator_reward_per_block_sol: GaugeVec,
    sol_watched_validator_vote_efficiency_pct: GaugeVec,
    sol_watched_validator_last_vote_slot_lag: GaugeVec,
    sol_watched_validator_root_slot_lag: GaugeVec,
    sol_watched_validator_commission_pct: GaugeVec,
    sol_watched_validator_blocks_produced: GaugeVec,
    sol_watched_validator_blocks_missed: GaugeVec,
    sol_watched_validator_block_production_rate: GaugeVec,
    sol_watched_validator_future_leader_slots: GaugeVec,
    sol_watched_validator_next_leader_slot: IntGaugeVec,
    sol_watched_validator_slots_until_next_leader: IntGaugeVec,
    sol_watched_validator_seconds_until_next_leader: GaugeVec,
}

impl Metrics {
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
        let sol_estimated_epoch_end_unixtime = GaugeVec::new(
            Opts::new(
                "sol_estimated_epoch_end_unixtime",
                "Estimated Unix time when the current epoch will end",
            ),
            &["network"],
        )?;
        let sol_watcher_up = IntGaugeVec::new(
            Opts::new("sol_watcher_up", "Whether the watcher process is running"),
            &["network"],
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
        register(&registry, &sol_estimated_epoch_end_unixtime)?;
        register(&registry, &sol_watcher_up)?;
        register(&registry, &sol_rpc_up)?;
        register(&registry, &sol_ws_up)?;
        register(&registry, &sol_watcher_config_info)?;
        register(&registry, &sol_connector_info)?;
        register(&registry, &sol_connector_feature_info)?;
        register(&registry, &sol_mode_enabled)?;
        register(&registry, &sol_mode_up)?;
        register(&registry, &sol_mode_command_route_info)?;
        register(&registry, &sol_mode_command_source_info)?;
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
            sol_estimated_epoch_end_unixtime,
            sol_watcher_up,
            sol_rpc_up,
            sol_ws_up,
            sol_watcher_config_info,
            sol_connector_info,
            sol_connector_feature_info,
            sol_mode_enabled,
            sol_mode_up,
            sol_mode_command_route_info,
            sol_mode_command_source_info,
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
            sol_watched_validator_next_leader_slot,
            sol_watched_validator_slots_until_next_leader,
            sol_watched_validator_seconds_until_next_leader,
        })
    }

    pub fn render(&self) -> Result<String> {
        let metric_families = self.registry.gather();
        let mut output = Vec::new();
        TextEncoder::new()
            .encode(&metric_families, &mut output)
            .context("failed to encode Prometheus metrics")?;

        String::from_utf8(output).context("metrics were not valid UTF-8")
    }

    pub fn set_watcher_up(&self, network: &str, is_up: bool) {
        self.sol_watcher_up.reset();
        self.sol_watcher_up
            .with_label_values(&[network])
            .set(i64::from(is_up));
    }

    pub fn set_rpc_up(&self, network: &str, is_up: bool) {
        self.sol_rpc_up.reset();
        self.sol_rpc_up
            .with_label_values(&[network])
            .set(i64::from(is_up));
    }

    pub fn set_ws_up(&self, network: &str, is_up: bool) {
        self.sol_ws_up.reset();
        self.sol_ws_up
            .with_label_values(&[network])
            .set(i64::from(is_up));
    }

    pub fn set_mode_up(&self, network: &str, mode: ModeId, is_up: bool) {
        self.sol_mode_up
            .with_label_values(&[network, mode.as_str()])
            .set(i64::from(is_up));
    }

    pub fn begin_poll(&self, config: &Config, sources: &SourceChain) {
        self.update_runtime_config(config, sources);
    }

    fn update_runtime_config(&self, config: &Config, sources: &SourceChain) {
        let connectors = &config.connectors;
        let ws_configured = connectors.iter().any(ConnectorConfig::has_ws);
        let transport_mode = if ws_configured {
            "poll-http-and-ws"
        } else {
            "poll-http"
        };

        self.sol_watcher_config_info.reset();
        self.sol_connector_info.reset();
        self.sol_connector_feature_info.reset();
        self.sol_mode_enabled.reset();
        self.sol_mode_command_route_info.reset();
        self.sol_watcher_config_info
            .with_label_values(&[&config.network, transport_mode, bool_label(ws_configured)])
            .set(1);

        for (index, connector) in connectors.iter().enumerate() {
            let declaration_order = (index + 1).to_string();
            self.sol_connector_info
                .with_label_values(&[
                    &config.network,
                    connector.name.as_str(),
                    connector.provider.as_str(),
                    declaration_order.as_str(),
                    solana_rpc_compatibility_label(connector.solana_rpc_compatibility),
                    bool_label(connector.has_rpc()),
                    bool_label(connector.has_ws()),
                ])
                .set(1);
            for feature in &connector.features {
                self.sol_connector_feature_info
                    .with_label_values(&[
                        &config.network,
                        connector.name.as_str(),
                        connector.provider.as_str(),
                        feature.as_str(),
                    ])
                    .set(1);
            }
        }

        for mode in ModeId::ALL {
            let enabled = config.modes.enabled(mode);
            self.sol_mode_enabled
                .with_label_values(&[&config.network, mode.as_str()])
                .set(i64::from(enabled));
        }

        for route in sources.routes() {
            let precedence = route.precedence.to_string();
            self.sol_mode_command_route_info
                .with_label_values(&[
                    &config.network,
                    route.mode.as_str(),
                    route.command.as_str(),
                    route.connector,
                    route.provider,
                    route.transport.as_str(),
                    solana_rpc_compatibility_label(route.solana_rpc_compatibility),
                    solana_rpc_compatibility_label(
                        route.command.source_requirement().solana_rpc_compatibility,
                    ),
                    feature_label(route.feature),
                    precedence.as_str(),
                    bool_label(route.available),
                ])
                .set(1);
        }
    }

    pub fn update_snapshot(&self, snapshot: &Snapshot) {
        self.sol_mode_command_source_info.reset();
        self.sol_mode_up.reset();
        self.reset_dynamic_metrics();
        self.sol_slot
            .with_label_values(&[&snapshot.network])
            .set(snapshot.slot as i64);
        self.sol_epoch
            .with_label_values(&[&snapshot.network])
            .set(snapshot.epoch as i64);
        if let Some(sample_epoch) = snapshot.vote_credits_sample_epoch {
            self.sol_vote_credits_sample_epoch
                .with_label_values(&[&snapshot.network])
                .set(sample_epoch as i64);
        }
        if let Some(sample_epoch) = snapshot.reward_sample_epoch {
            self.sol_reward_sample_epoch
                .with_label_values(&[&snapshot.network])
                .set(sample_epoch as i64);
        }
        self.sol_epoch_progress_pct
            .with_label_values(&[&snapshot.network])
            .set(snapshot.epoch_progress_pct);
        self.sol_epoch_slots_remaining
            .with_label_values(&[&snapshot.network])
            .set(snapshot.epoch_slots_remaining as i64);
        if let Some(slot_time) = snapshot.average_slot_time_seconds {
            self.sol_average_slot_time_seconds
                .with_label_values(&[&snapshot.network])
                .set(slot_time);
        }
        if let Some(epoch_end) = snapshot.estimated_epoch_end_unixtime {
            self.sol_estimated_epoch_end_unixtime
                .with_label_values(&[&snapshot.network])
                .set(epoch_end);
        }

        for status in &snapshot.mode_statuses {
            self.set_mode_up(&snapshot.network, status.mode, status.up);
        }

        for source in &snapshot.data_sources {
            let precedence = source.precedence.to_string();
            self.sol_mode_command_source_info
                .with_label_values(&[
                    &snapshot.network,
                    source.mode.as_str(),
                    source.command.as_str(),
                    source.connector.as_ref(),
                    source.provider.as_ref(),
                    source.transport.as_str(),
                    solana_rpc_compatibility_label(source.solana_rpc_compatibility),
                    solana_rpc_compatibility_label(
                        source.command.source_requirement().solana_rpc_compatibility,
                    ),
                    feature_label(source.feature),
                    precedence.as_str(),
                ])
                .set(1);
        }
        if let Some(economics) = &snapshot.economics {
            let labels = [
                snapshot.network.as_str(),
                economics.identity.as_str(),
                economics.vote_account.as_str(),
            ];
            self.sol_economics_identity_balance_sol
                .with_label_values(&labels)
                .set(lamports_to_sol(economics.identity_balance_lamports));
            self.sol_economics_vote_account_balance_sol
                .with_label_values(&labels)
                .set(lamports_to_sol(economics.vote_account_balance_lamports));
            if let Some(rent_exempt_minimum_lamports) =
                economics.vote_account_rent_exempt_minimum_lamports
            {
                self.sol_economics_vote_account_rent_exempt_minimum_sol
                    .with_label_values(&labels)
                    .set(lamports_to_sol(rent_exempt_minimum_lamports));
            }
            if let Some(vat_reserve_lamports) = economics.vat_reserve_lamports {
                self.sol_economics_vat_reserve_sol
                    .with_label_values(&labels)
                    .set(lamports_to_sol(vat_reserve_lamports));
            }
            if let Some(vat_epochs_available) = economics.vat_epochs_available {
                self.sol_economics_vat_epochs_available
                    .with_label_values(&labels)
                    .set(vat_epochs_available as i64);
            }
            if let Some(vat_next_epoch_funded) = economics.vat_next_epoch_funded {
                self.sol_economics_vat_next_epoch_funded
                    .with_label_values(&labels)
                    .set(i64::from(vat_next_epoch_funded));
            }
        }

        for (scope, metrics) in &snapshot.scopes {
            let network = snapshot.network.as_str();

            self.sol_validator_status_count
                .with_label_values(&[scope, "current", network])
                .set(metrics.current_count as f64);
            self.sol_validator_status_count
                .with_label_values(&[scope, "delinquent", network])
                .set(metrics.delinquent_count as f64);

            self.sol_validator_status_stake_sol
                .with_label_values(&[scope, "current", network])
                .set(lamports_to_sol(metrics.current_stake_lamports));
            self.sol_validator_status_stake_sol
                .with_label_values(&[scope, "delinquent", network])
                .set(lamports_to_sol(metrics.delinquent_stake_lamports));

            self.sol_activated_stake_sol
                .with_label_values(&[scope, network])
                .set(lamports_to_sol(metrics.activated_stake_lamports()));
            self.sol_delinquent_validator_count
                .with_label_values(&[scope, network])
                .set(metrics.delinquent_count as f64);
            self.sol_delinquent_stake_sol
                .with_label_values(&[scope, network])
                .set(lamports_to_sol(metrics.delinquent_stake_lamports));
            if let Some(vote_credits) = metrics.latest_epoch_vote_credits {
                self.sol_vote_credits_latest_epoch
                    .with_label_values(&[scope, network])
                    .set(vote_credits as f64);
            }
            self.sol_epoch_reward_sol
                .with_label_values(&[scope, network])
                .set(lamports_to_sol(metrics.latest_epoch_reward_lamports));
            self.sol_rewarded_blocks_latest_epoch
                .with_label_values(&[scope, network])
                .set(metrics.latest_epoch_rewarded_blocks_produced as f64);
            self.sol_reward_per_block_sol
                .with_label_values(&[scope, network])
                .set(reward_per_block_sol(
                    metrics.latest_epoch_reward_lamports,
                    metrics.latest_epoch_rewarded_blocks_produced,
                ));
            if let Some(vote_efficiency_pct) = metrics.vote_efficiency_pct {
                self.sol_vote_efficiency_pct
                    .with_label_values(&[scope, network])
                    .set(vote_efficiency_pct);
            }
            self.sol_last_vote_slot_lag
                .with_label_values(&[scope, network])
                .set(metrics.last_vote_slot_lag);
            self.sol_root_slot_lag
                .with_label_values(&[scope, network])
                .set(metrics.root_slot_lag);
            self.sol_average_commission_pct
                .with_label_values(&[scope, network])
                .set(metrics.average_commission_pct);
            self.sol_blocks_produced
                .with_label_values(&[scope, network])
                .set(metrics.blocks_produced as f64);
            self.sol_blocks_missed
                .with_label_values(&[scope, network])
                .set(metrics.blocks_missed as f64);
            self.sol_block_production_rate
                .with_label_values(&[scope, network])
                .set(metrics.block_production_rate);
            self.sol_future_leader_slots
                .with_label_values(&[scope, network])
                .set(metrics.future_leader_slots as f64);
        }

        for validator in &snapshot.watched_validators {
            self.update_watched_validator_metrics(&snapshot.network, validator);
        }
    }

    fn reset_dynamic_metrics(&self) {
        self.sol_vote_credits_sample_epoch.reset();
        self.sol_reward_sample_epoch.reset();
        self.sol_average_slot_time_seconds.reset();
        self.sol_estimated_epoch_end_unixtime.reset();
        self.sol_economics_identity_balance_sol.reset();
        self.sol_economics_vote_account_balance_sol.reset();
        self.sol_economics_vote_account_rent_exempt_minimum_sol
            .reset();
        self.sol_economics_vat_reserve_sol.reset();
        self.sol_economics_vat_epochs_available.reset();
        self.sol_economics_vat_next_epoch_funded.reset();
        self.sol_validator_status_count.reset();
        self.sol_validator_status_stake_sol.reset();
        self.sol_activated_stake_sol.reset();
        self.sol_delinquent_validator_count.reset();
        self.sol_delinquent_stake_sol.reset();
        self.sol_vote_credits_latest_epoch.reset();
        self.sol_epoch_reward_sol.reset();
        self.sol_rewarded_blocks_latest_epoch.reset();
        self.sol_reward_per_block_sol.reset();
        self.sol_vote_efficiency_pct.reset();
        self.sol_last_vote_slot_lag.reset();
        self.sol_root_slot_lag.reset();
        self.sol_average_commission_pct.reset();
        self.sol_blocks_produced.reset();
        self.sol_blocks_missed.reset();
        self.sol_block_production_rate.reset();
        self.sol_future_leader_slots.reset();
        self.sol_watched_validator_info.reset();
        self.sol_watched_validator_scope_info.reset();
        self.sol_watched_validator_activated_stake_sol.reset();
        self.sol_watched_validator_vote_credits_latest_epoch.reset();
        self.sol_watched_validator_epoch_reward_sol.reset();
        self.sol_watched_validator_rewarded_blocks_latest_epoch
            .reset();
        self.sol_watched_validator_reward_per_block_sol.reset();
        self.sol_watched_validator_vote_efficiency_pct.reset();
        self.sol_watched_validator_last_vote_slot_lag.reset();
        self.sol_watched_validator_root_slot_lag.reset();
        self.sol_watched_validator_commission_pct.reset();
        self.sol_watched_validator_blocks_produced.reset();
        self.sol_watched_validator_blocks_missed.reset();
        self.sol_watched_validator_block_production_rate.reset();
        self.sol_watched_validator_future_leader_slots.reset();
        self.sol_watched_validator_next_leader_slot.reset();
        self.sol_watched_validator_slots_until_next_leader.reset();
        self.sol_watched_validator_seconds_until_next_leader.reset();
    }

    fn update_watched_validator_metrics(&self, network: &str, validator: &WatchedValidatorMetrics) {
        let labels = watched_validator_labels(network, validator);

        self.sol_watched_validator_info
            .with_label_values(&labels)
            .set(1);
        self.sol_watched_validator_activated_stake_sol
            .with_label_values(&labels)
            .set(lamports_to_sol(validator.activated_stake_lamports));
        if let Some(vote_credits) = validator.latest_epoch_vote_credits {
            self.sol_watched_validator_vote_credits_latest_epoch
                .with_label_values(&labels)
                .set(vote_credits as f64);
        }
        self.sol_watched_validator_epoch_reward_sol
            .with_label_values(&labels)
            .set(lamports_to_sol(validator.latest_epoch_reward_lamports));
        self.sol_watched_validator_rewarded_blocks_latest_epoch
            .with_label_values(&labels)
            .set(validator.latest_epoch_rewarded_blocks_produced as f64);
        self.sol_watched_validator_reward_per_block_sol
            .with_label_values(&labels)
            .set(reward_per_block_sol(
                validator.latest_epoch_reward_lamports,
                validator.latest_epoch_rewarded_blocks_produced,
            ));
        if let Some(vote_efficiency_pct) = validator.vote_efficiency_pct {
            self.sol_watched_validator_vote_efficiency_pct
                .with_label_values(&labels)
                .set(vote_efficiency_pct);
        }
        self.sol_watched_validator_last_vote_slot_lag
            .with_label_values(&labels)
            .set(validator.last_vote_slot_lag as f64);
        self.sol_watched_validator_root_slot_lag
            .with_label_values(&labels)
            .set(validator.root_slot_lag as f64);
        self.sol_watched_validator_commission_pct
            .with_label_values(&labels)
            .set(f64::from(validator.commission_pct));
        self.sol_watched_validator_blocks_produced
            .with_label_values(&labels)
            .set(validator.blocks_produced as f64);
        self.sol_watched_validator_blocks_missed
            .with_label_values(&labels)
            .set(validator.blocks_missed as f64);
        self.sol_watched_validator_block_production_rate
            .with_label_values(&labels)
            .set(validator.block_production_rate);
        self.sol_watched_validator_future_leader_slots
            .with_label_values(&labels)
            .set(validator.future_leader_slots as f64);
        if let Some(next_leader_slot) = validator.next_leader_slot {
            self.sol_watched_validator_next_leader_slot
                .with_label_values(&labels)
                .set(next_leader_slot as i64);
        }
        if let Some(slots_until_next_leader) = validator.slots_until_next_leader {
            self.sol_watched_validator_slots_until_next_leader
                .with_label_values(&labels)
                .set(slots_until_next_leader as i64);
        }
        if let Some(seconds_until_next_leader) = validator.seconds_until_next_leader {
            self.sol_watched_validator_seconds_until_next_leader
                .with_label_values(&labels)
                .set(seconds_until_next_leader);
        }

        for scope in &validator.scopes {
            self.sol_watched_validator_scope_info
                .with_label_values(&[
                    network,
                    validator.identity.as_str(),
                    validator.vote_account.as_str(),
                    scope.as_str(),
                ])
                .set(1);
        }
    }
}

fn register<T>(registry: &Registry, collector: &T) -> Result<()>
where
    T: prometheus::core::Collector + Clone + 'static,
{
    registry
        .register(Box::new(collector.clone()))
        .context("failed to register Prometheus collector")
}

fn lamports_to_sol(lamports: u64) -> f64 {
    lamports as f64 / LAMPORTS_PER_SOL
}

fn reward_per_block_sol(reward_lamports: u64, produced_blocks: u64) -> f64 {
    if produced_blocks == 0 {
        return 0.0;
    }

    lamports_to_sol(reward_lamports) / produced_blocks as f64
}

fn bool_label(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn solana_rpc_compatibility_label(compatibility: Option<SolanaRpcCompatibility>) -> &'static str {
    compatibility.map_or("", SolanaRpcCompatibility::as_str)
}

fn feature_label(feature: Option<ProviderFeature>) -> &'static str {
    feature.map_or("", ProviderFeature::as_str)
}

fn watched_validator_labels<'a>(
    network: &'a str,
    validator: &'a WatchedValidatorMetrics,
) -> [&'a str; 4] {
    [
        network,
        validator.identity.as_str(),
        validator.vote_account.as_str(),
        validator.status.as_str(),
    ]
}

#[cfg(test)]
mod tests {
    use super::{bool_label, Metrics};
    use crate::{
        collector::{
            EconomicsMetrics, ModeStatus, ScopeMetrics, Snapshot, SnapshotDataSource,
            ValidatorStatus, WatchedValidatorMetrics,
        },
        config::{
            Config, ConnectorConfig, ConsensusCommands, ConsensusModeConfig, EconomicsCommands,
            EconomicsModeConfig, HealthCommands, HealthModeConfig, LeadershipCommands,
            LeadershipModeConfig, ModesConfig,
        },
        mode::{CommandId, ModeId, ProviderFeature, SolanaRpcCompatibility, Transport},
        source::SourceChain,
    };
    use anyhow::Result;
    use std::{collections::BTreeMap, sync::Arc};

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
        let sources = SourceChain::from_config(&config)?;
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
        assert!(
            rendered.contains("sol_mode_enabled{mode=\"economics\",network=\"mainnet-beta\"} 1")
        );
        assert!(rendered.contains("sol_mode_up{mode=\"leadership\",network=\"mainnet-beta\"} 1"));
        assert!(rendered.contains("sol_ws_up{network=\"mainnet-beta\"} 1"));
        assert!(rendered.contains("sol_vote_credits_sample_epoch{network=\"mainnet-beta\"} 41"));
        assert!(rendered.contains("sol_reward_sample_epoch{network=\"mainnet-beta\"} 41"));
        assert!(rendered.contains("sol_epoch_progress_pct{network=\"mainnet-beta\"} 34.5"));
        assert!(rendered.contains("sol_epoch_slots_remaining{network=\"mainnet-beta\"} 65"));
        assert!(rendered.contains("sol_average_slot_time_seconds{network=\"mainnet-beta\"} 0.4"));
        assert!(rendered
            .contains("sol_estimated_epoch_end_unixtime{network=\"mainnet-beta\"} 1800000000"));
        assert!(rendered.contains("sol_economics_identity_balance_sol{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 3"));
        assert!(rendered.contains("sol_economics_vote_account_balance_sol{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 2.5"));
        assert!(rendered.contains("sol_economics_vote_account_rent_exempt_minimum_sol{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 0.9"));
        assert!(rendered.contains("sol_economics_vat_reserve_sol{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 1.6"));
        assert!(rendered.contains("sol_economics_vat_epochs_available{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 2"));
        assert!(rendered.contains("sol_economics_vat_next_epoch_funded{identity=\"id-1\",network=\"mainnet-beta\",vote_account=\"vote-1\"} 1"));
        assert!(rendered.contains("sol_validator_status_count{network=\"mainnet-beta\",scope=\"scope:watched\",status=\"current\"} 1"));
        assert!(rendered
            .contains("sol_epoch_reward_sol{network=\"mainnet-beta\",scope=\"scope:watched\"} 6"));
        assert!(rendered.contains(
            "sol_reward_per_block_sol{network=\"mainnet-beta\",scope=\"scope:watched\"} 2"
        ));
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
        let sources = SourceChain::from_config(&config)?;
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
            ..Config::default()
        };
        let sources = SourceChain::from_config(&config)?;
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
}
