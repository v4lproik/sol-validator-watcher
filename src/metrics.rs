use crate::{
    collector::{Snapshot, ValidatorStatus, WatchedValidatorMetrics},
    config::Config,
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
    sol_watcher_up: IntGaugeVec,
    sol_rpc_up: IntGaugeVec,
    sol_ws_up: IntGaugeVec,
    sol_watcher_config_info: IntGaugeVec,
    sol_validator_status_count: GaugeVec,
    sol_validator_status_stake_sol: GaugeVec,
    sol_activated_stake_sol: GaugeVec,
    sol_delinquent_validator_count: GaugeVec,
    sol_delinquent_stake_sol: GaugeVec,
    sol_vote_credits_latest_epoch: GaugeVec,
    sol_vote_efficiency_pct: GaugeVec,
    sol_last_vote_slot_lag: GaugeVec,
    sol_root_slot_lag: GaugeVec,
    sol_average_commission_pct: GaugeVec,
    sol_blocks_produced: GaugeVec,
    sol_blocks_missed: GaugeVec,
    sol_block_production_rate: GaugeVec,
    sol_future_leader_slots: GaugeVec,
    sol_watched_validator_info: IntGaugeVec,
    sol_watched_validator_scope_info: IntGaugeVec,
    sol_watched_validator_activated_stake_sol: GaugeVec,
    sol_watched_validator_vote_credits_latest_epoch: GaugeVec,
    sol_watched_validator_vote_efficiency_pct: GaugeVec,
    sol_watched_validator_last_vote_slot_lag: GaugeVec,
    sol_watched_validator_root_slot_lag: GaugeVec,
    sol_watched_validator_commission_pct: GaugeVec,
    sol_watched_validator_blocks_produced: GaugeVec,
    sol_watched_validator_blocks_missed: GaugeVec,
    sol_watched_validator_block_production_rate: GaugeVec,
    sol_watched_validator_future_leader_slots: GaugeVec,
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
            Opts::new(
                "sol_watcher_config_info",
                "Watcher transport and validator mode configuration",
            ),
            &[
                "network",
                "transport_mode",
                "validator_mode",
                "validator_websocket_enabled",
                "ws_configured",
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

        register(&registry, &sol_slot)?;
        register(&registry, &sol_epoch)?;
        register(&registry, &sol_vote_credits_sample_epoch)?;
        register(&registry, &sol_watcher_up)?;
        register(&registry, &sol_rpc_up)?;
        register(&registry, &sol_ws_up)?;
        register(&registry, &sol_watcher_config_info)?;
        register(&registry, &sol_validator_status_count)?;
        register(&registry, &sol_validator_status_stake_sol)?;
        register(&registry, &sol_activated_stake_sol)?;
        register(&registry, &sol_delinquent_validator_count)?;
        register(&registry, &sol_delinquent_stake_sol)?;
        register(&registry, &sol_vote_credits_latest_epoch)?;
        register(&registry, &sol_vote_efficiency_pct)?;
        register(&registry, &sol_last_vote_slot_lag)?;
        register(&registry, &sol_root_slot_lag)?;
        register(&registry, &sol_average_commission_pct)?;
        register(&registry, &sol_blocks_produced)?;
        register(&registry, &sol_blocks_missed)?;
        register(&registry, &sol_block_production_rate)?;
        register(&registry, &sol_future_leader_slots)?;
        register(&registry, &sol_watched_validator_info)?;
        register(&registry, &sol_watched_validator_scope_info)?;
        register(&registry, &sol_watched_validator_activated_stake_sol)?;
        register(&registry, &sol_watched_validator_vote_credits_latest_epoch)?;
        register(&registry, &sol_watched_validator_vote_efficiency_pct)?;
        register(&registry, &sol_watched_validator_last_vote_slot_lag)?;
        register(&registry, &sol_watched_validator_root_slot_lag)?;
        register(&registry, &sol_watched_validator_commission_pct)?;
        register(&registry, &sol_watched_validator_blocks_produced)?;
        register(&registry, &sol_watched_validator_blocks_missed)?;
        register(&registry, &sol_watched_validator_block_production_rate)?;
        register(&registry, &sol_watched_validator_future_leader_slots)?;

        Ok(Self {
            registry,
            sol_slot,
            sol_epoch,
            sol_vote_credits_sample_epoch,
            sol_watcher_up,
            sol_rpc_up,
            sol_ws_up,
            sol_watcher_config_info,
            sol_validator_status_count,
            sol_validator_status_stake_sol,
            sol_activated_stake_sol,
            sol_delinquent_validator_count,
            sol_delinquent_stake_sol,
            sol_vote_credits_latest_epoch,
            sol_vote_efficiency_pct,
            sol_last_vote_slot_lag,
            sol_root_slot_lag,
            sol_average_commission_pct,
            sol_blocks_produced,
            sol_blocks_missed,
            sol_block_production_rate,
            sol_future_leader_slots,
            sol_watched_validator_info,
            sol_watched_validator_scope_info,
            sol_watched_validator_activated_stake_sol,
            sol_watched_validator_vote_credits_latest_epoch,
            sol_watched_validator_vote_efficiency_pct,
            sol_watched_validator_last_vote_slot_lag,
            sol_watched_validator_root_slot_lag,
            sol_watched_validator_commission_pct,
            sol_watched_validator_blocks_produced,
            sol_watched_validator_blocks_missed,
            sol_watched_validator_block_production_rate,
            sol_watched_validator_future_leader_slots,
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

    pub fn update_runtime_config(&self, config: &Config) {
        self.sol_watcher_config_info.reset();
        self.sol_watcher_config_info
            .with_label_values(&[
                &config.network,
                config.transport_mode.as_str(),
                config.validator_mode.as_str(),
                bool_label(config.validator_websocket_enabled),
                bool_label(config.ws_url.is_some()),
            ])
            .set(1);
    }

    pub fn update_snapshot(&self, snapshot: &Snapshot) {
        self.reset_dynamic_metrics();

        self.sol_slot
            .with_label_values(&[&snapshot.network])
            .set(snapshot.slot as i64);
        self.sol_epoch
            .with_label_values(&[&snapshot.network])
            .set(snapshot.epoch as i64);
        self.sol_vote_credits_sample_epoch
            .with_label_values(&[&snapshot.network])
            .set(
                snapshot
                    .vote_credits_sample_epoch
                    .map_or(0, |epoch| epoch as i64),
            );

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
                .set(lamports_to_sol(
                    metrics.current_stake_lamports + metrics.delinquent_stake_lamports,
                ));
            self.sol_delinquent_validator_count
                .with_label_values(&[scope, network])
                .set(metrics.delinquent_count as f64);
            self.sol_delinquent_stake_sol
                .with_label_values(&[scope, network])
                .set(lamports_to_sol(metrics.delinquent_stake_lamports));
            self.sol_vote_credits_latest_epoch
                .with_label_values(&[scope, network])
                .set(metrics.latest_epoch_vote_credits as f64);
            self.sol_vote_efficiency_pct
                .with_label_values(&[scope, network])
                .set(metrics.vote_efficiency_pct);
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
        self.sol_validator_status_count.reset();
        self.sol_validator_status_stake_sol.reset();
        self.sol_activated_stake_sol.reset();
        self.sol_delinquent_validator_count.reset();
        self.sol_delinquent_stake_sol.reset();
        self.sol_vote_credits_latest_epoch.reset();
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
        self.sol_watched_validator_vote_efficiency_pct.reset();
        self.sol_watched_validator_last_vote_slot_lag.reset();
        self.sol_watched_validator_root_slot_lag.reset();
        self.sol_watched_validator_commission_pct.reset();
        self.sol_watched_validator_blocks_produced.reset();
        self.sol_watched_validator_blocks_missed.reset();
        self.sol_watched_validator_block_production_rate.reset();
        self.sol_watched_validator_future_leader_slots.reset();
    }

    fn update_watched_validator_metrics(&self, network: &str, validator: &WatchedValidatorMetrics) {
        let status = validator_status_label(validator.status);
        let labels = &[
            network,
            validator.identity.as_str(),
            validator.vote_account.as_str(),
            status,
        ];

        self.sol_watched_validator_info
            .with_label_values(labels)
            .set(1);
        self.sol_watched_validator_activated_stake_sol
            .with_label_values(labels)
            .set(lamports_to_sol(validator.activated_stake_lamports));
        self.sol_watched_validator_vote_credits_latest_epoch
            .with_label_values(labels)
            .set(validator.latest_epoch_vote_credits as f64);
        self.sol_watched_validator_vote_efficiency_pct
            .with_label_values(labels)
            .set(validator.vote_efficiency_pct);
        self.sol_watched_validator_last_vote_slot_lag
            .with_label_values(labels)
            .set(validator.last_vote_slot_lag as f64);
        self.sol_watched_validator_root_slot_lag
            .with_label_values(labels)
            .set(validator.root_slot_lag as f64);
        self.sol_watched_validator_commission_pct
            .with_label_values(labels)
            .set(f64::from(validator.commission_pct));
        self.sol_watched_validator_blocks_produced
            .with_label_values(labels)
            .set(validator.blocks_produced as f64);
        self.sol_watched_validator_blocks_missed
            .with_label_values(labels)
            .set(validator.blocks_missed as f64);
        self.sol_watched_validator_block_production_rate
            .with_label_values(labels)
            .set(validator.block_production_rate);
        self.sol_watched_validator_future_leader_slots
            .with_label_values(labels)
            .set(validator.future_leader_slots as f64);

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

fn validator_status_label(status: ValidatorStatus) -> &'static str {
    match status {
        ValidatorStatus::Current => "current",
        ValidatorStatus::Delinquent => "delinquent",
    }
}

fn bool_label(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

#[cfg(test)]
mod tests {
    use super::Metrics;
    use crate::{
        collector::{ScopeMetrics, Snapshot, ValidatorStatus, WatchedValidatorMetrics},
        config::{Config, TransportMode, ValidatorMode},
    };
    use anyhow::Result;
    use std::collections::BTreeMap;

    #[test]
    fn renders_scoped_and_watched_validator_metrics() -> Result<()> {
        let metrics = Metrics::new()?;
        let snapshot = Snapshot {
            network: "mainnet-beta".to_string(),
            epoch: 42,
            slot: 1234,
            vote_credits_sample_epoch: Some(41),
            scopes: BTreeMap::from([(
                "scope:watched".to_string(),
                ScopeMetrics {
                    current_count: 1,
                    delinquent_count: 0,
                    current_stake_lamports: 1_500_000_000,
                    delinquent_stake_lamports: 0,
                    latest_epoch_vote_credits: 11,
                    vote_efficiency_pct: 91.5,
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
                latest_epoch_vote_credits: 11,
                vote_efficiency_pct: 91.5,
                last_vote_slot_lag: 4,
                root_slot_lag: 5,
                commission_pct: 8,
                blocks_produced: 3,
                blocks_missed: 1,
                block_production_rate: 75.0,
                future_leader_slots: 2,
            }],
        };

        metrics.set_watcher_up("mainnet-beta", true);
        metrics.set_rpc_up("mainnet-beta", true);
        metrics.set_ws_up("mainnet-beta", true);
        metrics.update_runtime_config(&Config {
            network: "mainnet-beta".to_string(),
            rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
            rpc_timeout_sec: 30,
            transport_mode: TransportMode::PollHttpAndWs,
            ws_url: Some("wss://api.mainnet-beta.solana.com".to_string()),
            ws_timeout_sec: 10,
            validator_mode: ValidatorMode::ConsensusAndRpc,
            validator_websocket_enabled: true,
            metrics_port: 8000,
            poll_interval_sec: 15,
            commitment: "confirmed".to_string(),
            delinquent_slot_distance: None,
            liveness_file: None,
            watched_validators: vec![],
        });
        metrics.update_snapshot(&snapshot);
        let rendered = metrics.render()?;

        assert!(rendered.contains("sol_watcher_config_info{network=\"mainnet-beta\",transport_mode=\"poll-http-and-ws\",validator_mode=\"consensus-and-rpc\",validator_websocket_enabled=\"true\",ws_configured=\"true\"} 1"));
        assert!(rendered.contains("sol_ws_up{network=\"mainnet-beta\"} 1"));
        assert!(rendered.contains("sol_validator_status_count{network=\"mainnet-beta\",scope=\"scope:watched\",status=\"current\"} 1"));
        assert!(rendered.contains("sol_watched_validator_info{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 1"));
        assert!(rendered.contains("sol_watched_validator_scope_info{identity=\"id-1\",network=\"mainnet-beta\",scope=\"region:sbg\",vote_account=\"vote-1\"} 1"));
        assert!(rendered.contains("sol_watched_validator_activated_stake_sol{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 1.5"));
        assert!(rendered.contains("sol_watched_validator_block_production_rate{identity=\"id-1\",network=\"mainnet-beta\",status=\"current\",vote_account=\"vote-1\"} 75"));

        Ok(())
    }

    #[test]
    fn reset_removes_stale_watched_validator_metrics() -> Result<()> {
        let metrics = Metrics::new()?;
        let network = "mainnet-beta";

        metrics.update_runtime_config(&Config {
            network: network.to_string(),
            rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
            rpc_timeout_sec: 30,
            transport_mode: TransportMode::PollHttpAndWs,
            ws_url: Some("wss://api.mainnet-beta.solana.com".to_string()),
            ws_timeout_sec: 10,
            validator_mode: ValidatorMode::Consensus,
            validator_websocket_enabled: true,
            metrics_port: 8000,
            poll_interval_sec: 15,
            commitment: "confirmed".to_string(),
            delinquent_slot_distance: None,
            liveness_file: None,
            watched_validators: vec![],
        });
        metrics.update_snapshot(&Snapshot {
            network: network.to_string(),
            epoch: 1,
            slot: 1,
            vote_credits_sample_epoch: Some(1),
            scopes: BTreeMap::new(),
            watched_validators: vec![WatchedValidatorMetrics {
                identity: "old-id".to_string(),
                vote_account: "old-vote".to_string(),
                status: ValidatorStatus::Current,
                scopes: vec!["scope:watched".to_string()],
                activated_stake_lamports: 1_000_000_000,
                latest_epoch_vote_credits: 1,
                vote_efficiency_pct: 100.0,
                last_vote_slot_lag: 0,
                root_slot_lag: 0,
                commission_pct: 0,
                blocks_produced: 1,
                blocks_missed: 0,
                block_production_rate: 100.0,
                future_leader_slots: 0,
            }],
        });

        metrics.update_snapshot(&Snapshot {
            network: network.to_string(),
            epoch: 2,
            slot: 2,
            vote_credits_sample_epoch: Some(2),
            scopes: BTreeMap::new(),
            watched_validators: vec![],
        });

        let rendered = metrics.render()?;
        assert!(!rendered.contains("old-id"));
        assert!(!rendered.contains("old-vote"));

        Ok(())
    }
}
