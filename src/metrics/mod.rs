//! Prometheus metric ownership and snapshot publication.
//!
//! This module owns the exported metric contract, including descriptors, labels,
//! reset behavior, rendering, and unit conversion.

use crate::{
    collector::{Snapshot, WatchedValidatorMetrics, MAX_UPCOMING_LEADER_SLOTS},
    config::{Config, ConnectorConfig},
    mode::{ModeId, ProviderFeature, SolanaRpcCompatibility},
    source::SourceChain,
    watcher_state::RpcResponseCounts,
};
use anyhow::{Context, Result};
use prometheus::{
    Encoder, GaugeVec, HistogramVec, IntCounterVec, IntGaugeVec, Registry, TextEncoder,
};
mod registration;

const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;
const SECONDS_PER_HOUR: f64 = 3_600.0;
const UPCOMING_LEADER_POSITION_LABELS: [&str; MAX_UPCOMING_LEADER_SLOTS] = [
    "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12", "13", "14", "15", "16",
];

/// Single owner of Prometheus collectors and their reset/update semantics.
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
    sol_cluster_transactions_per_second: GaugeVec,
    sol_cluster_vote_transactions_per_second: GaugeVec,
    sol_cluster_non_vote_transactions_per_second: GaugeVec,
    sol_cluster_slots_per_second: GaugeVec,
    sol_estimated_epoch_end_unixtime: GaugeVec,
    sol_watcher_up: IntGaugeVec,
    sol_watcher_ready: IntGaugeVec,
    sol_watcher_poll_round_count: IntGaugeVec,
    sol_watcher_clean_poll_streak: IntGaugeVec,
    sol_watcher_poll_interval_seconds: IntGaugeVec,
    sol_last_successful_snapshot_timestamp_seconds: GaugeVec,
    sol_snapshot_duration_seconds: HistogramVec,
    sol_snapshot_failures_total: IntCounterVec,
    sol_rpc_http_response_class_count: IntGaugeVec,
    sol_rpc_http_response_status_count: IntGaugeVec,
    sol_rpc_request_failure_count: IntGaugeVec,
    sol_rpc_request_duration_seconds: HistogramVec,
    sol_rpc_request_failures_total: IntCounterVec,
    sol_rpc_up: IntGaugeVec,
    sol_ws_up: IntGaugeVec,
    sol_ws_probe_failures_total: IntCounterVec,
    sol_watcher_config_info: IntGaugeVec,
    sol_connector_info: IntGaugeVec,
    sol_connector_feature_info: IntGaugeVec,
    sol_mode_enabled: IntGaugeVec,
    sol_mode_up: IntGaugeVec,
    sol_mode_command_route_info: IntGaugeVec,
    sol_mode_command_source_info: IntGaugeVec,
    sol_node_info: IntGaugeVec,
    sol_node_health: IntGaugeVec,
    sol_node_catchup_distance_slots: IntGaugeVec,
    sol_node_block_height: IntGaugeVec,
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
    sol_economics_block_fee_reward_sol: GaugeVec,
    sol_economics_block_fee_reward_slot: IntGaugeVec,
    sol_economics_recent_block_fee_reward_average_sol: GaugeVec,
    sol_economics_recent_block_fee_reward_sample_count: IntGaugeVec,
    sol_economics_jito_mev_reward_sol: GaugeVec,
    sol_economics_jito_mev_commission_pct: GaugeVec,
    sol_economics_jito_mev_reward_epoch: IntGaugeVec,
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
    sol_watched_validator_current_epoch_scheduled_leader_slots: IntGaugeVec,
    sol_watched_validator_scheduled_leader_slots_per_hour: GaugeVec,
    sol_watched_validator_upcoming_leader_slot: IntGaugeVec,
    sol_watched_validator_next_leader_slot: IntGaugeVec,
    sol_watched_validator_slots_until_next_leader: IntGaugeVec,
    sol_watched_validator_seconds_until_next_leader: GaugeVec,
}

impl Metrics {
    /// Encodes the current registry in Prometheus text exposition format.
    pub fn render(&self) -> Result<String> {
        let metric_families = self.registry.gather();
        let mut output = Vec::new();
        TextEncoder::new()
            .encode(&metric_families, &mut output)
            .context("failed to encode Prometheus metrics")?;

        String::from_utf8(output).context("metrics were not valid UTF-8")
    }
    /// Clears transport and snapshot observations for a newly accepted configuration.
    pub fn reset_runtime_observations(&self, network: &str) {
        self.sol_last_successful_snapshot_timestamp_seconds.reset();
        self.sol_snapshot_duration_seconds.reset();
        self.sol_snapshot_failures_total.reset();
        self.sol_rpc_request_duration_seconds.reset();
        self.sol_rpc_request_failures_total.reset();
        self.sol_ws_probe_failures_total.reset();
        self.sol_snapshot_failures_total
            .with_label_values(&[network])
            .inc_by(0);
    }

    /// Records one successfully published snapshot.
    pub fn record_snapshot_success(
        &self,
        network: &str,
        duration: std::time::Duration,
        completed_at_unix_seconds: f64,
    ) {
        self.sol_last_successful_snapshot_timestamp_seconds
            .with_label_values(&[network])
            .set(completed_at_unix_seconds);
        self.sol_snapshot_duration_seconds
            .with_label_values(&[network, "success"])
            .observe(duration.as_secs_f64());
    }

    /// Records one polling round that failed before publishing a complete snapshot.
    pub fn record_snapshot_failure(&self, network: &str, duration: std::time::Duration) {
        self.sol_snapshot_duration_seconds
            .with_label_values(&[network, "failure"])
            .observe(duration.as_secs_f64());
        self.sol_snapshot_failures_total
            .with_label_values(&[network])
            .inc();
    }

    /// Records one bounded RPC request duration.
    pub fn record_rpc_request_duration(
        &self,
        network: &str,
        connector: &str,
        method: &str,
        duration: std::time::Duration,
    ) {
        self.sol_rpc_request_duration_seconds
            .with_label_values(&[network, connector, method])
            .observe(duration.as_secs_f64());
    }

    /// Records an RPC failure at any transport, protocol, or semantic boundary.
    pub fn record_rpc_request_failure(&self, network: &str, connector: &str, method: &str) {
        self.sol_rpc_request_failures_total
            .with_label_values(&[network, connector, method])
            .inc();
    }

    /// Replaces the process-liveness series with the current network value.
    pub fn set_watcher_up(&self, network: &str, is_up: bool) {
        self.sol_watcher_up.reset();
        self.sol_watcher_up
            .with_label_values(&[network])
            .set(i64::from(is_up));
    }

    /// Publishes one coherent watcher-state snapshot.
    ///
    /// Response counters are cumulative since process start; stale network series
    /// are removed before the new values are written.
    pub fn update_watcher_state(
        &self,
        network: &str,
        ready: bool,
        completed_poll_rounds: u64,
        clean_poll_rounds: u8,
        responses: RpcResponseCounts,
    ) {
        self.sol_watcher_ready.reset();
        self.sol_watcher_poll_round_count.reset();
        self.sol_watcher_clean_poll_streak.reset();
        self.sol_rpc_http_response_class_count.reset();
        self.sol_rpc_http_response_status_count.reset();
        self.sol_rpc_request_failure_count.reset();

        self.sol_watcher_ready
            .with_label_values(&[network])
            .set(i64::from(ready));
        self.sol_watcher_poll_round_count
            .with_label_values(&[network])
            .set(metric_count(completed_poll_rounds));
        self.sol_watcher_clean_poll_streak
            .with_label_values(&[network])
            .set(i64::from(clean_poll_rounds));

        for (status_class, count) in [
            ("2xx", responses.successful_2xx),
            ("3xx", responses.redirection_3xx),
            ("4xx", responses.client_error_4xx),
            ("5xx", responses.server_error_5xx),
            ("other", responses.other),
        ] {
            self.sol_rpc_http_response_class_count
                .with_label_values(&[network, status_class])
                .set(metric_count(count));
        }
        for (status_code, count) in [
            ("200", responses.status_200),
            ("403", responses.status_403),
            ("429", responses.status_429),
            ("500", responses.status_500),
        ] {
            self.sol_rpc_http_response_status_count
                .with_label_values(&[network, status_code])
                .set(metric_count(count));
        }
        self.sol_rpc_request_failure_count
            .with_label_values(&[network])
            .set(metric_count(responses.failed_attempts));
    }

    /// Replaces the latest HTTP RPC health series.
    pub fn set_rpc_up(&self, network: &str, is_up: bool) {
        self.sol_rpc_up.reset();
        self.sol_rpc_up
            .with_label_values(&[network])
            .set(i64::from(is_up));
    }

    /// Replaces the latest WebSocket health series.
    pub fn set_ws_up(&self, network: &str, is_up: bool) {
        self.sol_ws_up.reset();
        self.sol_ws_up
            .with_label_values(&[network])
            .set(i64::from(is_up));
    }

    /// Records one failed WebSocket reachability probe.
    pub fn record_ws_probe_failure(&self, network: &str, connector: &str) {
        self.sol_ws_probe_failures_total
            .with_label_values(&[network, connector])
            .inc();
    }

    /// Sets collection health for one mode.
    pub fn set_mode_up(&self, network: &str, mode: ModeId, is_up: bool) {
        self.sol_mode_up
            .with_label_values(&[network, mode.as_str()])
            .set(i64::from(is_up));
    }

    /// Publishes the active connector and command-route configuration for a poll.
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
        self.sol_watcher_poll_interval_seconds.reset();
        self.sol_watcher_config_info
            .with_label_values(&[&config.network, transport_mode, bool_label(ws_configured)])
            .set(1);
        self.sol_watcher_poll_interval_seconds
            .with_label_values(&[&config.network])
            .set(metric_count(config.poll_interval_sec));

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
                    bool_label(connector.has_provider_api()),
                ])
                .set(1);
            if connector.has_ws() {
                self.sol_ws_probe_failures_total
                    .with_label_values(&[&config.network, connector.name.as_str()])
                    .inc_by(0);
            }
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

    /// Replaces dynamic collection metrics with one completed snapshot.
    ///
    /// Optional values that are absent from the snapshot remain absent rather than
    /// retaining a stale series or being exported as zero.
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
        if let Some(performance) = snapshot.cluster_performance {
            let network = &[snapshot.network.as_str()];
            if let Some(slot_time) = performance.average_slot_time_seconds {
                self.sol_average_slot_time_seconds
                    .with_label_values(network)
                    .set(slot_time);
            }
            self.sol_cluster_transactions_per_second
                .with_label_values(network)
                .set(performance.transactions_per_second);
            if let Some(vote_transactions_per_second) = performance.vote_transactions_per_second {
                self.sol_cluster_vote_transactions_per_second
                    .with_label_values(network)
                    .set(vote_transactions_per_second);
            }
            if let Some(non_vote_transactions_per_second) =
                performance.non_vote_transactions_per_second
            {
                self.sol_cluster_non_vote_transactions_per_second
                    .with_label_values(network)
                    .set(non_vote_transactions_per_second);
            }
            self.sol_cluster_slots_per_second
                .with_label_values(network)
                .set(performance.slots_per_second);
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
        if let Some(node) = &snapshot.node {
            let feature_set = node
                .feature_set
                .map(|value| value.to_string())
                .unwrap_or_default();
            self.sol_node_info
                .with_label_values(&[
                    &snapshot.network,
                    &node.identity,
                    &node.version,
                    feature_set.as_str(),
                    node.voting_status.as_str(),
                ])
                .set(1);
            self.sol_node_health
                .with_label_values(&[&snapshot.network, &node.identity])
                .set(i64::from(node.health.is_healthy()));
            self.sol_node_block_height
                .with_label_values(&[&snapshot.network, &node.identity])
                .set(metric_count(node.block_height));
            if let Some(distance) = node.health.catchup_distance_slots() {
                self.sol_node_catchup_distance_slots
                    .with_label_values(&[&snapshot.network, &node.identity])
                    .set(metric_count(distance));
            }
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
            if let Some(block_rewards) = economics.recent_block_fee_rewards {
                self.sol_economics_block_fee_reward_sol
                    .with_label_values(&labels)
                    .set(lamports_to_sol(block_rewards.latest_reward_lamports));
                self.sol_economics_block_fee_reward_slot
                    .with_label_values(&labels)
                    .set(metric_count(block_rewards.latest_slot));
                self.sol_economics_recent_block_fee_reward_sample_count
                    .with_label_values(&labels)
                    .set(metric_count(block_rewards.sampled_block_count));
                if block_rewards.sampled_block_count > 0 {
                    self.sol_economics_recent_block_fee_reward_average_sol
                        .with_label_values(&labels)
                        .set(
                            lamports_to_sol(block_rewards.total_reward_lamports)
                                / block_rewards.sampled_block_count as f64,
                        );
                }
            }
            if let Some(jito_reward) = economics.jito_mev_reward {
                self.sol_economics_jito_mev_reward_sol
                    .with_label_values(&labels)
                    .set(lamports_to_sol(jito_reward.reward_lamports));
                self.sol_economics_jito_mev_commission_pct
                    .with_label_values(&labels)
                    .set(f64::from(jito_reward.commission_bps) / 100.0);
                self.sol_economics_jito_mev_reward_epoch
                    .with_label_values(&labels)
                    .set(metric_count(jito_reward.epoch));
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

        let average_slot_time_seconds = snapshot
            .cluster_performance
            .and_then(|performance| performance.average_slot_time_seconds);
        for validator in &snapshot.watched_validators {
            self.update_watched_validator_metrics(
                &snapshot.network,
                snapshot.epoch_slots,
                average_slot_time_seconds,
                validator,
            );
        }
    }

    fn reset_dynamic_metrics(&self) {
        self.sol_vote_credits_sample_epoch.reset();
        self.sol_reward_sample_epoch.reset();
        self.sol_average_slot_time_seconds.reset();
        self.sol_cluster_transactions_per_second.reset();
        self.sol_cluster_vote_transactions_per_second.reset();
        self.sol_cluster_non_vote_transactions_per_second.reset();
        self.sol_cluster_slots_per_second.reset();
        self.sol_estimated_epoch_end_unixtime.reset();
        self.sol_node_info.reset();
        self.sol_node_health.reset();
        self.sol_node_catchup_distance_slots.reset();
        self.sol_node_block_height.reset();
        self.sol_economics_identity_balance_sol.reset();
        self.sol_economics_vote_account_balance_sol.reset();
        self.sol_economics_vote_account_rent_exempt_minimum_sol
            .reset();
        self.sol_economics_vat_reserve_sol.reset();
        self.sol_economics_vat_epochs_available.reset();
        self.sol_economics_vat_next_epoch_funded.reset();
        self.sol_economics_block_fee_reward_sol.reset();
        self.sol_economics_block_fee_reward_slot.reset();
        self.sol_economics_recent_block_fee_reward_average_sol
            .reset();
        self.sol_economics_recent_block_fee_reward_sample_count
            .reset();
        self.sol_economics_jito_mev_reward_sol.reset();
        self.sol_economics_jito_mev_commission_pct.reset();
        self.sol_economics_jito_mev_reward_epoch.reset();
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
        self.sol_watched_validator_current_epoch_scheduled_leader_slots
            .reset();
        self.sol_watched_validator_scheduled_leader_slots_per_hour
            .reset();
        self.sol_watched_validator_upcoming_leader_slot.reset();
        self.sol_watched_validator_next_leader_slot.reset();
        self.sol_watched_validator_slots_until_next_leader.reset();
        self.sol_watched_validator_seconds_until_next_leader.reset();
    }

    fn update_watched_validator_metrics(
        &self,
        network: &str,
        epoch_slots: u64,
        average_slot_time_seconds: Option<f64>,
        validator: &WatchedValidatorMetrics,
    ) {
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
        if let Some(scheduled_leader_slots) = validator.current_epoch_scheduled_leader_slots {
            self.sol_watched_validator_current_epoch_scheduled_leader_slots
                .with_label_values(&labels)
                .set(metric_count(scheduled_leader_slots));
            if let Some(slots_per_hour) = scheduled_leader_slots_per_hour(
                scheduled_leader_slots,
                epoch_slots,
                average_slot_time_seconds,
            ) {
                self.sol_watched_validator_scheduled_leader_slots_per_hour
                    .with_label_values(&labels)
                    .set(slots_per_hour);
            }
        }
        for (slot, position) in validator
            .upcoming_leader_slots
            .iter()
            .zip(UPCOMING_LEADER_POSITION_LABELS)
        {
            self.sol_watched_validator_upcoming_leader_slot
                .with_label_values(&[
                    network,
                    validator.identity.as_str(),
                    validator.vote_account.as_str(),
                    validator.status.as_str(),
                    position,
                ])
                .set(metric_count(*slot));
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

fn metric_count(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
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

fn scheduled_leader_slots_per_hour(
    scheduled_leader_slots: u64,
    epoch_slots: u64,
    average_slot_time_seconds: Option<f64>,
) -> Option<f64> {
    let average_slot_time_seconds =
        average_slot_time_seconds.filter(|seconds| seconds.is_finite() && *seconds > 0.0)?;
    if epoch_slots == 0 {
        return None;
    }

    Some(
        scheduled_leader_slots as f64 * SECONDS_PER_HOUR
            / (epoch_slots as f64 * average_slot_time_seconds),
    )
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
mod tests;
