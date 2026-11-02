use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use sol_validator_watcher::{
    collector::{
        aggregate_records, EpochCreditsSample, RewardSample, ScopeMetrics, Snapshot,
        SnapshotDataSource, ValidatorRecord, ValidatorStatus, WatchedValidatorMetrics,
    },
    config::{
        Config, ConnectorConfig, ConnectorKind, EconomicsConfig, EconomicsMode, ValidatorMode,
    },
    metrics::Metrics,
    provider::{CommandId, ProviderDriver, COMMAND_COUNT},
    source::SourceChain,
};
use std::collections::BTreeMap;

fn benchmark_aggregate_records(c: &mut Criterion) {
    let records = sample_validator_records(2_000, 250);

    c.bench_function("collector/aggregate_records_2000", |b| {
        b.iter(|| {
            aggregate_records(
                "mainnet-beta".to_string(),
                512,
                12_345_678,
                black_box(&records),
            )
        })
    });
}

fn benchmark_metrics_update_runtime_config(c: &mut Criterion) {
    let metrics = Metrics::new().expect("metrics should initialize");
    let config = sample_config(48);

    c.bench_function("metrics/update_runtime_config_48_connectors", |b| {
        b.iter(|| metrics.update_runtime_config(black_box(&config)))
    });
}

fn benchmark_source_chain_build(c: &mut Criterion) {
    let config = sample_config(48);

    c.bench_function("source/build_chain_48_connectors", |b| {
        b.iter(|| {
            SourceChain::from_config(black_box(&config)).expect("source chain should initialize")
        })
    });
}

fn benchmark_metrics_update_snapshot(c: &mut Criterion) {
    let metrics = Metrics::new().expect("metrics should initialize");
    let snapshot = sample_snapshot(1_500, 200, 12);

    c.bench_function("metrics/update_snapshot_1500_validators", |b| {
        b.iter_batched(
            || snapshot.clone(),
            |snapshot| metrics.update_snapshot(black_box(&snapshot)),
            BatchSize::SmallInput,
        )
    });
}

fn sample_config(connector_count: usize) -> Config {
    let connectors = (0..connector_count)
        .map(|index| ConnectorConfig {
            name: format!("connector-{index}"),
            kind: match index % 3 {
                0 => ConnectorKind::StandardValidator,
                1 => ConnectorKind::HeliusEnhancedRpc,
                _ => ConnectorKind::HeliusEnhancedWss,
            },
            rpc_url: (index % 3 != 2).then(|| format!("https://rpc-{index}.example")),
            ws_url: (index % 2 == 0).then(|| format!("wss://ws-{index}.example")),
            priority: (connector_count - index) as i32,
            ..ConnectorConfig::default()
        })
        .collect::<Vec<_>>();

    Config {
        network: "mainnet-beta".to_string(),
        connectors,
        validator_mode: ValidatorMode::ConsensusAndRpc,
        validator_websocket_enabled: true,
        economics: EconomicsConfig {
            mode: EconomicsMode::Disabled,
            validator: None,
        },
        ..Config::default()
    }
}

fn sample_snapshot(
    scope_validator_count: usize,
    watched_validator_count: usize,
    metric_source_count: usize,
) -> Snapshot {
    let scopes = BTreeMap::from([
        (
            "scope:all-network".to_string(),
            ScopeMetrics {
                current_count: scope_validator_count as u64,
                delinquent_count: (scope_validator_count / 10) as u64,
                current_stake_lamports: 10_000_000_000_000,
                delinquent_stake_lamports: 500_000_000_000,
                latest_epoch_vote_credits: 42_000,
                latest_epoch_reward_lamports: 12_000_000_000,
                latest_epoch_rewarded_blocks_produced: 2_000,
                vote_efficiency_pct: 0.96,
                last_vote_slot_lag: 3.0,
                root_slot_lag: 5.0,
                average_commission_pct: 5.0,
                blocks_produced: 20_000,
                blocks_missed: 500,
                block_production_rate: 0.975,
                future_leader_slots: 2_500,
            },
        ),
        (
            "scope:watched".to_string(),
            ScopeMetrics {
                current_count: watched_validator_count as u64,
                delinquent_count: 0,
                current_stake_lamports: 2_000_000_000_000,
                delinquent_stake_lamports: 0,
                latest_epoch_vote_credits: 8_000,
                latest_epoch_reward_lamports: 3_500_000_000,
                latest_epoch_rewarded_blocks_produced: 600,
                vote_efficiency_pct: 0.98,
                last_vote_slot_lag: 1.0,
                root_slot_lag: 2.0,
                average_commission_pct: 4.0,
                blocks_produced: 4_000,
                blocks_missed: 100,
                block_production_rate: 0.975,
                future_leader_slots: 500,
            },
        ),
    ]);

    let watched_validators = (0..watched_validator_count)
        .map(|index| WatchedValidatorMetrics {
            identity: format!("id-{index}"),
            vote_account: format!("vote-{index}"),
            status: if index % 10 == 0 {
                ValidatorStatus::Delinquent
            } else {
                ValidatorStatus::Current
            },
            scopes: vec![
                "scope:all-network".to_string(),
                "scope:watched".to_string(),
                format!("operator:{}", index % 8),
            ],
            activated_stake_lamports: 1_000_000_000 + index as u64,
            latest_epoch_vote_credits: 200 + index as u64,
            vote_efficiency_pct: 0.9,
            last_vote_slot_lag: 1,
            root_slot_lag: 2,
            commission_pct: (index % 10) as u8,
            latest_epoch_reward_lamports: 100_000_000,
            latest_epoch_rewarded_blocks_produced: 16,
            blocks_produced: 120,
            blocks_missed: 4,
            block_production_rate: 0.9677,
            future_leader_slots: 12,
        })
        .collect::<Vec<_>>();

    let data_sources = (0..metric_source_count)
        .map(|index| {
            let enhanced = index % 2 != 0;
            SnapshotDataSource {
                command: CommandId::ALL[index % COMMAND_COUNT],
                connector: format!("connector-{}", index % 6).into(),
                kind: if enhanced {
                    ConnectorKind::HeliusEnhancedRpc
                } else {
                    ConnectorKind::StandardValidator
                },
                driver: if enhanced {
                    ProviderDriver::EnhancedRpc
                } else {
                    ProviderDriver::StandardRpc
                },
                effective_priority: index as i32,
            }
        })
        .collect::<Vec<_>>();

    Snapshot {
        network: "mainnet-beta".to_string(),
        epoch: 512,
        slot: 12_345_678,
        vote_credits_sample_epoch: Some(511),
        reward_sample_epoch: Some(511),
        scopes,
        watched_validators,
        data_sources,
    }
}

fn sample_validator_records(total: usize, watched: usize) -> Vec<ValidatorRecord> {
    (0..total)
        .map(|index| {
            let mut scopes = vec![
                "scope:all-network".to_string(),
                "scope:network".to_string(),
                format!("operator:{}", index % 16),
            ];
            if index < watched {
                scopes.push("scope:watched".to_string());
            }

            ValidatorRecord {
                identity: format!("id-{index}"),
                vote_account: format!("vote-{index}"),
                status: if index % 17 == 0 {
                    ValidatorStatus::Delinquent
                } else {
                    ValidatorStatus::Current
                },
                activated_stake_lamports: 1_000_000_000 + (index as u64 * 1_000),
                commission: (index % 10) as u8,
                last_vote: 12_345_600 - (index as u64 % 32),
                root_slot: 12_345_580 - (index as u64 % 64),
                latest_epoch_credits: Some(EpochCreditsSample {
                    epoch: 511,
                    earned: 100 + index as u64 % 32,
                }),
                latest_epoch_reward: Some(RewardSample {
                    epoch: 511,
                    amount_lamports: 50_000_000 + index as u64,
                }),
                latest_epoch_rewarded_blocks_produced: 5 + index as u64 % 4,
                current_epoch_leader_slots: 16 + index as u64 % 8,
                current_epoch_blocks_produced: 12 + index as u64 % 8,
                future_leader_slots: 4 + index as u64 % 3,
                scopes,
            }
        })
        .collect()
}

criterion_group!(
    perf,
    benchmark_aggregate_records,
    benchmark_metrics_update_runtime_config,
    benchmark_source_chain_build,
    benchmark_metrics_update_snapshot
);
criterion_main!(perf);
