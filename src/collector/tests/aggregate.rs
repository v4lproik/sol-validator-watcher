use super::*;
#[test]
fn aggregates_by_scope() -> Result<()> {
    let records = vec![
        ValidatorRecord {
            identity: "id-1".to_string(),
            vote_account: "vote-1".to_string(),
            status: ValidatorStatus::Current,
            activated_stake_lamports: 10,
            commission: 5,
            last_vote: 95,
            root_slot: 94,
            latest_epoch_credits: Some(EpochCreditsSample {
                epoch: 9,
                earned: 10,
            }),
            latest_epoch_reward: Some(RewardSample {
                epoch: 9,
                amount_lamports: 9_000_000_000,
            }),
            latest_epoch_rewarded_blocks_produced: 3,
            current_epoch_leader_slots: 4,
            current_epoch_blocks_produced: 3,
            future_leader_slots: 2,
            next_leader_slot: Some(105),
            slots_until_next_leader: Some(5),
            seconds_until_next_leader: Some(2.0),
            scopes: vec!["scope:watched".to_string(), "scope:all-network".to_string()],
        },
        ValidatorRecord {
            identity: "id-2".to_string(),
            vote_account: "vote-2".to_string(),
            status: ValidatorStatus::Delinquent,
            activated_stake_lamports: 5,
            commission: 10,
            last_vote: 90,
            root_slot: 88,
            latest_epoch_credits: Some(EpochCreditsSample {
                epoch: 9,
                earned: 5,
            }),
            latest_epoch_reward: Some(RewardSample {
                epoch: 9,
                amount_lamports: 2_000_000_000,
            }),
            latest_epoch_rewarded_blocks_produced: 1,
            current_epoch_leader_slots: 2,
            current_epoch_blocks_produced: 1,
            future_leader_slots: 1,
            next_leader_slot: None,
            slots_until_next_leader: None,
            seconds_until_next_leader: None,
            scopes: vec!["scope:network".to_string(), "scope:all-network".to_string()],
        },
    ];

    let snapshot = aggregate_records("mainnet-beta".to_string(), 10, 100, &records);
    let watched = snapshot
        .scopes
        .get("scope:watched")
        .ok_or_else(|| anyhow!("missing watched scope"))?;
    let all_network = snapshot
        .scopes
        .get("scope:all-network")
        .ok_or_else(|| anyhow!("missing all-network scope"))?;

    assert_eq!(snapshot.vote_credits_sample_epoch, Some(9));
    assert_eq!(snapshot.reward_sample_epoch, Some(9));
    assert_eq!(
        watched,
        &ScopeMetrics {
            current_count: 1,
            delinquent_count: 0,
            current_stake_lamports: 10,
            delinquent_stake_lamports: 0,
            latest_epoch_vote_credits: Some(10),
            latest_epoch_reward_lamports: 9_000_000_000,
            latest_epoch_rewarded_blocks_produced: 3,
            vote_efficiency_pct: Some(100.0),
            last_vote_slot_lag: 5.0,
            root_slot_lag: 6.0,
            average_commission_pct: 5.0,
            blocks_produced: 3,
            blocks_missed: 1,
            block_production_rate: 75.0,
            future_leader_slots: 2,
        }
    );

    assert_eq!(all_network.current_count, 1);
    assert_eq!(all_network.delinquent_count, 1);
    assert_eq!(all_network.latest_epoch_vote_credits, Some(15));
    assert!(all_network
        .vote_efficiency_pct
        .is_some_and(|efficiency| efficiency > 80.0));
    assert_eq!(
        snapshot.watched_validators,
        vec![WatchedValidatorMetrics {
            identity: "id-1".to_string(),
            vote_account: "vote-1".to_string(),
            status: ValidatorStatus::Current,
            scopes: vec!["scope:watched".to_string(), "scope:all-network".to_string()],
            activated_stake_lamports: 10,
            latest_epoch_vote_credits: Some(10),
            latest_epoch_reward_lamports: 9_000_000_000,
            latest_epoch_rewarded_blocks_produced: 3,
            vote_efficiency_pct: Some(100.0),
            last_vote_slot_lag: 5,
            root_slot_lag: 6,
            commission_pct: 5,
            blocks_produced: 3,
            blocks_missed: 1,
            block_production_rate: 75.0,
            future_leader_slots: 2,
            next_leader_slot: Some(105),
            slots_until_next_leader: Some(5),
            seconds_until_next_leader: Some(2.0),
        }]
    );

    Ok(())
}

#[test]
fn watched_validator_metrics_omit_efficiency_without_matching_epoch() {
    let record = ValidatorRecord {
        identity: "id-1".to_string(),
        vote_account: "vote-1".to_string(),
        status: ValidatorStatus::Current,
        activated_stake_lamports: 10,
        commission: 7,
        last_vote: 98,
        root_slot: 97,
        latest_epoch_credits: Some(EpochCreditsSample {
            epoch: 4,
            earned: 12,
        }),
        latest_epoch_reward: None,
        latest_epoch_rewarded_blocks_produced: 0,
        current_epoch_leader_slots: 0,
        current_epoch_blocks_produced: 0,
        future_leader_slots: 3,
        next_leader_slot: Some(103),
        slots_until_next_leader: Some(3),
        seconds_until_next_leader: Some(1.2),
        scopes: vec!["scope:watched".to_string()],
    };

    let watched = build_watched_validator_metrics(&record, 100, Some(20), Some(5), None);
    assert_eq!(
        watched,
        WatchedValidatorMetrics {
            identity: "id-1".to_string(),
            vote_account: "vote-1".to_string(),
            status: ValidatorStatus::Current,
            scopes: vec!["scope:watched".to_string()],
            activated_stake_lamports: 10,
            latest_epoch_vote_credits: None,
            latest_epoch_reward_lamports: 0,
            latest_epoch_rewarded_blocks_produced: 0,
            vote_efficiency_pct: None,
            last_vote_slot_lag: 2,
            root_slot_lag: 3,
            commission_pct: 7,
            blocks_produced: 0,
            blocks_missed: 0,
            block_production_rate: 0.0,
            future_leader_slots: 3,
            next_leader_slot: Some(103),
            slots_until_next_leader: Some(3),
            seconds_until_next_leader: Some(1.2),
        }
    );
}

#[test]
fn ratio_returns_zero_for_empty_denominator() {
    assert_eq!(ratio(0, 0), 0.0);
    assert_eq!(ratio(5, 0), 0.0);
    assert_eq!(ratio(3, 4), 75.0);
}

#[test]
fn build_record_uses_block_production_schedules_and_labels() {
    let config = Config {
        network: "devnet".to_string(),
        connectors: vec![ConnectorConfig {
            name: "primary".to_string(),
            provider: "solana-validator".to_string(),
            features: Vec::new(),
            rpc_url: Some("https://rpc.example".to_string()),
            ws_url: Some("wss://rpc.example".to_string()),
            ..ConnectorConfig::default()
        }],
        delinquent_slot_distance: Some(128),
        watched_validators: vec![WatchedValidatorConfig {
            identity: None,
            vote_account: Some("vote-1".to_string()),
            labels: vec!["operator:test".to_string()],
        }],
        ..Config::default()
    };
    let matcher = LabelMatcher::from_config(&config);
    let mut by_identity = HashMap::new();
    let mut reward_epoch_by_identity = HashMap::new();
    let mut current_schedule = HashMap::new();
    let mut next_schedule = HashMap::new();
    let mut latest_epoch_rewards = HashMap::new();

    by_identity.insert("id-1".to_string(), (5, 4));
    reward_epoch_by_identity.insert("id-1".to_string(), (4, 3));
    current_schedule.insert("id-1".to_string(), vec![5, 10, 15]);
    next_schedule.insert("id-1".to_string(), vec![1, 2]);
    latest_epoch_rewards.insert(
        "vote-1".to_string(),
        RewardSample {
            epoch: 9,
            amount_lamports: 12_000_000_000,
        },
    );

    let context = RecordBuildContext {
        matcher: &matcher,
        current_epoch: 9,
        current_slot: 110,
        epoch_start_slot: 100,
        next_epoch_start_slot: 200,
        average_slot_time_seconds: Some(0.5),
        by_identity: &by_identity,
        reward_epoch_by_identity: &reward_epoch_by_identity,
        current_schedule: Some(&current_schedule),
        next_schedule: Some(&next_schedule),
        latest_epoch_rewards: &latest_epoch_rewards,
    };

    let record = build_record(
        VoteAccountInfo {
            vote_pubkey: "vote-1".to_string(),
            node_pubkey: "id-1".to_string(),
            activated_stake: 42,
            commission: 7,
            epoch_credits: vec![(8, 20, 12), (9, 35, 20), (u64::MAX, u64::MAX, u64::MAX)],
            last_vote: 109,
            root_slot: 108,
        },
        ValidatorStatus::Current,
        &context,
    );

    assert_eq!(record.current_epoch_leader_slots, 5);
    assert_eq!(record.current_epoch_blocks_produced, 4);
    assert_eq!(record.latest_epoch_rewarded_blocks_produced, 3);
    assert_eq!(record.future_leader_slots, 4);
    assert_eq!(record.next_leader_slot, Some(110));
    assert_eq!(record.slots_until_next_leader, Some(0));
    assert_eq!(record.seconds_until_next_leader, Some(0.0));
    assert_eq!(
        record.latest_epoch_credits,
        Some(EpochCreditsSample {
            epoch: 9,
            earned: 15,
        })
    );
    assert_eq!(
        record.latest_epoch_reward,
        Some(RewardSample {
            epoch: 9,
            amount_lamports: 12_000_000_000,
        })
    );
    assert!(record.scopes.contains(&"scope:watched".to_string()));
    assert!(record.scopes.contains(&"operator:test".to_string()));
}

#[test]
fn build_record_defaults_missing_block_data_to_zero() {
    let config = Config {
        network: "devnet".to_string(),
        connectors: vec![ConnectorConfig {
            name: "primary".to_string(),
            provider: "solana-validator".to_string(),
            features: Vec::new(),
            rpc_url: Some("https://rpc.example".to_string()),
            ..ConnectorConfig::default()
        }],
        ..Config::default()
    };
    let matcher = LabelMatcher::from_config(&config);
    let by_identity = HashMap::new();
    let reward_epoch_by_identity = HashMap::new();
    let latest_epoch_rewards = HashMap::new();

    let context = RecordBuildContext {
        matcher: &matcher,
        current_epoch: 1,
        current_slot: 50,
        epoch_start_slot: 40,
        next_epoch_start_slot: 140,
        average_slot_time_seconds: None,
        by_identity: &by_identity,
        reward_epoch_by_identity: &reward_epoch_by_identity,
        current_schedule: None,
        next_schedule: None,
        latest_epoch_rewards: &latest_epoch_rewards,
    };

    let record = build_record(
        VoteAccountInfo {
            vote_pubkey: "vote-2".to_string(),
            node_pubkey: "id-2".to_string(),
            activated_stake: 0,
            commission: 0,
            epoch_credits: vec![],
            last_vote: 49,
            root_slot: 48,
        },
        ValidatorStatus::Delinquent,
        &context,
    );

    assert_eq!(record.current_epoch_leader_slots, 0);
    assert_eq!(record.current_epoch_blocks_produced, 0);
    assert_eq!(record.future_leader_slots, 0);
    assert_eq!(record.next_leader_slot, None);
    assert_eq!(record.slots_until_next_leader, None);
    assert_eq!(record.seconds_until_next_leader, None);
    assert_eq!(record.latest_epoch_credits, None);
    assert_eq!(record.latest_epoch_reward, None);
    assert!(record.scopes.contains(&"scope:network".to_string()));
}

#[test]
fn reward_epoch_block_range_skips_pruned_history() {
    assert!(reward_epoch_block_range(100, 20, 81).is_none());
    assert!(matches!(
        reward_epoch_block_range(100, 20, 80),
        Some(SlotRange {
            first_slot: 80,
            last_slot: 99,
        })
    ));
    assert!(reward_epoch_block_range(0, 20, 0).is_none());
}

#[test]
fn economics_target_resolves_vote_account_from_configured_identity() -> Result<()> {
    let config = Config {
        modes: ModesConfig {
            economics: EconomicsModeConfig {
                enabled: true,
                validator: Some(crate::config::WatchedValidatorConfig {
                    identity: Some("id-1".to_string()),
                    vote_account: None,
                    labels: vec![],
                }),
                ..EconomicsModeConfig::default()
            },
            ..ModesConfig::default()
        },
        ..Config::default()
    };
    let vote_accounts = VoteAccounts {
        current: vec![],
        delinquent: vec![VoteAccountInfo {
            vote_pubkey: "vote-1".to_string(),
            node_pubkey: "id-1".to_string(),
            activated_stake: 0,
            commission: 0,
            epoch_credits: vec![],
            last_vote: 0,
            root_slot: 0,
        }],
    };

    assert_eq!(
        resolve_economics_target(&config, &vote_accounts)?,
        EconomicsTarget {
            identity: "id-1".to_string(),
            vote_account: "vote-1".to_string(),
        }
    );
    Ok(())
}

#[test]
fn economics_target_rejects_mismatched_identity_and_vote_account() {
    let config = Config {
        modes: ModesConfig {
            economics: EconomicsModeConfig {
                enabled: true,
                validator: Some(crate::config::WatchedValidatorConfig {
                    identity: Some("id-1".to_string()),
                    vote_account: Some("wrong-vote".to_string()),
                    labels: vec![],
                }),
                ..EconomicsModeConfig::default()
            },
            ..ModesConfig::default()
        },
        ..Config::default()
    };
    let vote_accounts = VoteAccounts {
        current: vec![VoteAccountInfo {
            vote_pubkey: "vote-1".to_string(),
            node_pubkey: "id-1".to_string(),
            activated_stake: 0,
            commission: 0,
            epoch_credits: vec![],
            last_vote: 0,
            root_slot: 0,
        }],
        delinquent: vec![],
    };

    let error = resolve_economics_target(&config, &vote_accounts)
        .err()
        .map(|error| error.to_string());
    assert_eq!(
        error.as_deref(),
        Some("configured economics validator is not present in getVoteAccounts")
    );
}

#[test]
fn economics_metrics_preserve_rent_and_vat_boundaries() {
    let metrics = build_economics_metrics(
        &EconomicsTarget {
            identity: "id-1".to_string(),
            vote_account: "vote-1".to_string(),
        },
        AccountBalances {
            identity_lamports: 3_000_000_000,
            vote_account_lamports: 2_500_000_000,
            vote_account_rent_exempt_minimum_lamports: Some(900_000_000),
        },
        None,
    );

    assert_eq!(metrics.vat_reserve_lamports, Some(1_600_000_000));
    assert_eq!(metrics.vat_epochs_available, Some(2));
    assert_eq!(metrics.vat_next_epoch_funded, Some(true));
}

#[test]
fn epoch_end_estimate_requires_average_slot_time() {
    assert_eq!(
        estimated_epoch_end_unixtime_at(100, Some(0.4), 1_700_000_000.0),
        Some(1_700_000_040.0)
    );
    assert_eq!(
        estimated_epoch_end_unixtime_at(100, None, 1_700_000_000.0),
        None
    );
}

#[test]
fn aggregate_records_emits_zeroed_builtin_scopes_for_empty_clusters() -> Result<()> {
    let snapshot = aggregate_records("localnet".to_string(), 0, 42, &[]);

    let all_network = snapshot
        .scopes
        .get("scope:all-network")
        .ok_or_else(|| anyhow!("missing all-network scope"))?;
    let network = snapshot
        .scopes
        .get("scope:network")
        .ok_or_else(|| anyhow!("missing network scope"))?;
    let watched = snapshot
        .scopes
        .get("scope:watched")
        .ok_or_else(|| anyhow!("missing watched scope"))?;

    assert_eq!(snapshot.network, "localnet");
    assert_eq!(snapshot.slot, 42);
    assert_eq!(all_network, &ScopeMetrics::default());
    assert_eq!(network, &ScopeMetrics::default());
    assert_eq!(watched, &ScopeMetrics::default());
    Ok(())
}
