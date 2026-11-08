//! Transactional orchestration of one collected watcher snapshot.
//!
//! Each command resolves through its own source route. Provenance is recorded
//! only after a command succeeds, and optional absence remains explicit.

use super::{
    aggregate::{
        aggregate_records, apply_validator_funding_metrics, build_economics_metrics, build_record,
        estimated_epoch_end_unixtime, ratio, resolve_economics_target, EconomicsTarget,
        RecordBuildContext,
    },
    command,
    model::{
        AccountBalanceTarget, AccountBalances, GossipObservation, ModeStatus, NodeVotingStatus,
        Snapshot, ValidatorStatus, WatchedValidatorMetrics,
    },
    rpc::{BlockProduction, ClusterNode, VoteAccounts, MAX_RECENT_BLOCK_FEE_REWARD_SLOTS},
};
use crate::{
    config::Config,
    labels::LabelMatcher,
    mode::{CommandId, ModeId},
    source::SourceChain,
};
use anyhow::{bail, Result};
use std::collections::BinaryHeap;

/// Exercises every required economics command before economics collection starts.
///
/// This validates the configured target against live vote accounts and confirms
/// that balance, prior-epoch reward, and block-production routes are usable.
pub async fn ensure_economics_supported(sources: &SourceChain, config: &Config) -> Result<()> {
    if !config.modes.economics.enabled {
        return Ok(());
    }

    let epoch_command = command::GetEpochInfo;
    let vote_accounts_command = command::GetVoteAccounts {
        delinquent_slot_distance: config.delinquent_slot_distance,
    };
    let (epoch_info, vote_accounts) = tokio::join!(
        sources.required(&epoch_command),
        sources.required(&vote_accounts_command),
    );
    let epoch_info = epoch_info?;
    let vote_accounts = vote_accounts?;
    let target = resolve_economics_target(config, &vote_accounts.value)?;
    let matcher = LabelMatcher::from_config(config);
    let mut account_targets = watched_account_balance_targets(&vote_accounts.value, &matcher);
    include_economics_target(&mut account_targets, &target);
    let balances = sources
        .required(&command::GetAccountBalances {
            targets: &account_targets,
        })
        .await?;
    if !economics_target_has_funding(&target, &account_targets, &balances.value) {
        bail!("configured economics target account was not found");
    }

    let Some(reward_epoch) = epoch_info.value.epoch.checked_sub(1) else {
        return Ok(());
    };
    let current_slot = epoch_info.value.absolute_slot;
    let epoch_start_slot = current_slot.saturating_sub(epoch_info.value.slot_index);
    sources
        .required(&command::GetInflationRewards {
            addresses: std::slice::from_ref(&target.vote_account),
            epoch: reward_epoch,
        })
        .await?;
    sources
        .required(&command::GetRewardEpochBlockProduction {
            epoch_start_slot,
            slots_in_epoch: epoch_info.value.slots_in_epoch,
        })
        .await?;
    if !config.modes.route(CommandId::JitoMevRewards).is_empty() {
        sources
            .required(&command::GetJitoMevReward {
                vote_account: &target.vote_account,
                epoch: reward_epoch,
            })
            .await?;
    }

    Ok(())
}

/// Collects one coherent snapshot through the configured per-command source routes.
///
/// Consensus inputs and enabled economics inputs are required. Leadership inputs
/// are optional as a group: missing data marks leadership down while preserving
/// the rest of the snapshot.
pub async fn collect_snapshot(sources: &SourceChain, config: &Config) -> Result<Snapshot> {
    let epoch_command = command::GetEpochInfo;
    let vote_accounts_command = command::GetVoteAccounts {
        delinquent_slot_distance: config.delinquent_slot_distance,
    };
    let (epoch_info, vote_accounts, cluster_nodes, recent_performance, node_status) = tokio::join!(
        sources.required(&epoch_command),
        sources.required(&vote_accounts_command),
        sources.optional(&command::GetClusterNodes),
        sources.optional(&command::GetRecentPerformanceSamples),
        sources.optional(&command::GetNodeStatus),
    );
    let epoch_info = epoch_info?;
    let vote_accounts = vote_accounts?;
    let mut data_sources = vec![
        epoch_info.source.clone().into(),
        vote_accounts.source.clone().into(),
    ];
    let node = node_status.map(|resolved| {
        data_sources.push(resolved.source.into());
        let voting_status = if vote_accounts
            .value
            .current
            .iter()
            .any(|account| account.node_pubkey == resolved.value.identity)
        {
            NodeVotingStatus::Current
        } else if vote_accounts
            .value
            .delinquent
            .iter()
            .any(|account| account.node_pubkey == resolved.value.identity)
        {
            NodeVotingStatus::Delinquent
        } else {
            NodeVotingStatus::NotFound
        };
        resolved.value.with_voting_status(voting_status)
    });
    let cluster_performance = recent_performance.and_then(|resolved| {
        data_sources.push(resolved.source.into());
        resolved.value
    });
    let average_slot_time_seconds = cluster_performance
        .as_ref()
        .and_then(|performance| performance.average_slot_time_seconds);

    let current_slot = epoch_info.value.absolute_slot;
    let epoch_start_slot = current_slot.saturating_sub(epoch_info.value.slot_index);
    let next_epoch_start_slot = epoch_start_slot.saturating_add(epoch_info.value.slots_in_epoch);
    let reward_epoch = epoch_info.value.epoch.checked_sub(1);
    let economics_enabled = config.modes.economics.enabled;
    let economics_target = economics_enabled
        .then(|| resolve_economics_target(config, &vote_accounts.value))
        .transpose()?;
    let reward_vote_accounts = economics_enabled.then(|| {
        vote_accounts
            .value
            .current
            .iter()
            .chain(vote_accounts.value.delinquent.iter())
            .map(|info| info.vote_pubkey.clone())
            .collect::<Vec<_>>()
    });
    let current_schedule_command = command::GetLeaderSchedule { slot: current_slot };
    let next_schedule_command = command::GetNextLeaderSchedule {
        slot: next_epoch_start_slot,
    };

    let (current_block_production, current_schedule, next_schedule, economics_outputs) = tokio::join!(
        sources.optional(&command::GetCurrentBlockProduction),
        sources.optional(&current_schedule_command),
        sources.optional(&next_schedule_command),
        async {
            let Some(target) = economics_target.as_ref() else {
                return Ok::<_, anyhow::Error>((None, None, None, None));
            };

            let (Some(epoch), Some(addresses)) = (reward_epoch, reward_vote_accounts.as_deref())
            else {
                return Ok((None, None, None, None));
            };
            let inflation_rewards_command = command::GetInflationRewards { addresses, epoch };
            let block_production_command = command::GetRewardEpochBlockProduction {
                epoch_start_slot,
                slots_in_epoch: epoch_info.value.slots_in_epoch,
            };
            let jito_mev_reward_command = command::GetJitoMevReward {
                vote_account: &target.vote_account,
                epoch,
            };
            let trillium_mev_capture_command = command::GetTrilliumMevCapture {
                vote_account: &target.vote_account,
            };
            let (inflation_rewards, block_production, jito_mev_reward, trillium_mev_capture) = tokio::join!(
                sources.required(&inflation_rewards_command),
                sources.required(&block_production_command),
                sources.optional(&jito_mev_reward_command),
                sources.optional(&trillium_mev_capture_command),
            );
            Ok((
                Some(inflation_rewards?),
                Some(block_production?),
                jito_mev_reward,
                trillium_mev_capture,
            ))
        },
    );

    let (inflation_rewards, reward_epoch_block_production, jito_mev_reward, trillium_mev_capture) =
        economics_outputs?;
    let leadership_up =
        current_block_production.is_some() && current_schedule.is_some() && next_schedule.is_some();

    let block_production = match current_block_production {
        Some(resolved) => {
            data_sources.push(resolved.source.into());
            resolved.value
        }
        None => BlockProduction::default(),
    };
    let current_schedule = current_schedule.and_then(|resolved| {
        data_sources.push(resolved.source.into());
        resolved.value
    });
    let next_schedule = next_schedule.and_then(|resolved| {
        data_sources.push(resolved.source.into());
        resolved.value
    });
    let jito_mev_reward = jito_mev_reward.and_then(|resolved| {
        data_sources.push(resolved.source.into());
        resolved.value
    });
    let trillium_mev_capture = trillium_mev_capture.and_then(|resolved| {
        data_sources.push(resolved.source.into());
        resolved.value
    });
    let recent_block_fee_rewards = match (
        economics_target.as_ref(),
        current_schedule.as_ref(),
        config
            .modes
            .route(CommandId::RecentBlockFeeRewards)
            .is_empty(),
    ) {
        (Some(target), Some(schedule), false) => {
            let slots = schedule
                .get(&target.identity)
                .map(|relative_slots| {
                    recent_completed_leader_slots(relative_slots, epoch_start_slot, current_slot)
                })
                .unwrap_or_default();
            if slots.is_empty() {
                None
            } else {
                let command = command::GetRecentBlockFeeRewards {
                    slots: &slots,
                    identity: &target.identity,
                };
                match sources.optional(&command).await {
                    Some(resolved) => {
                        data_sources.push(resolved.source.into());
                        resolved.value
                    }
                    None => None,
                }
            }
        }
        _ => None,
    };
    let economics = economics_target.as_ref().map(|target| {
        build_economics_metrics(
            target,
            jito_mev_reward,
            trillium_mev_capture,
            recent_block_fee_rewards,
        )
    });
    let inflation_rewards = inflation_rewards
        .map(|resolved| {
            data_sources.push(resolved.source.into());
            resolved.value
        })
        .unwrap_or_default();
    let reward_epoch_block_production = reward_epoch_block_production
        .map(|resolved| {
            data_sources.push(resolved.source.into());
            resolved.value
        })
        .unwrap_or_default();

    let matcher = LabelMatcher::from_config(config);
    let build_context = RecordBuildContext {
        matcher: &matcher,
        current_slot,
        current_epoch: epoch_info.value.epoch,
        epoch_start_slot,
        next_epoch_start_slot,
        average_slot_time_seconds,
        by_identity: &block_production.by_identity,
        reward_epoch_by_identity: &reward_epoch_block_production.by_identity,
        current_schedule: current_schedule.as_ref(),
        next_schedule: next_schedule.as_ref(),
        latest_epoch_rewards: &inflation_rewards,
    };
    let records = vote_accounts
        .value
        .current
        .into_iter()
        .map(|info| build_record(info, ValidatorStatus::Current, &build_context))
        .chain(
            vote_accounts
                .value
                .delinquent
                .into_iter()
                .map(|info| build_record(info, ValidatorStatus::Delinquent, &build_context)),
        )
        .collect::<Vec<_>>();

    let epoch_slots_remaining = epoch_info
        .value
        .slots_in_epoch
        .saturating_sub(epoch_info.value.slot_index);
    let mode_statuses = [
        ModeStatus {
            mode: ModeId::Consensus,
            up: true,
        },
        ModeStatus {
            mode: ModeId::Leadership,
            up: leadership_up,
        },
    ]
    .into_iter()
    .chain(economics_enabled.then_some(ModeStatus {
        mode: ModeId::Economics,
        up: true,
    }))
    .collect();
    let mut snapshot = aggregate_records(
        config.network.clone(),
        epoch_info.value.epoch,
        current_slot,
        &records,
    );
    if let Some(resolved) = cluster_nodes {
        data_sources.push(resolved.source.into());
        apply_gossip_observations(&mut snapshot.watched_validators, resolved.value);
    }
    if economics_enabled {
        let Some(target) = economics_target.as_ref() else {
            bail!("economics mode is enabled without a resolved target");
        };
        let watched_validator_count = snapshot.watched_validators.len();
        let mut account_targets = snapshot
            .watched_validators
            .iter()
            .map(|validator| AccountBalanceTarget {
                identity: validator.identity.as_str(),
                vote_account: validator.vote_account.as_str(),
            })
            .collect::<Vec<_>>();
        include_economics_target(&mut account_targets, target);
        let balances = sources
            .required(&command::GetAccountBalances {
                targets: &account_targets,
            })
            .await?;
        if !economics_target_has_funding(target, &account_targets, &balances.value) {
            bail!("configured economics target account was not found");
        }
        drop(account_targets);
        data_sources.push(balances.source.into());
        let mut funding = balances.value;
        funding.truncate(watched_validator_count);
        apply_validator_funding_metrics(&mut snapshot.watched_validators, funding)?;
    }

    Ok(Snapshot {
        epoch_slots: epoch_info.value.slots_in_epoch,
        epoch_progress_pct: ratio(epoch_info.value.slot_index, epoch_info.value.slots_in_epoch),

        epoch_slots_remaining,
        cluster_performance,
        estimated_epoch_end_unixtime: estimated_epoch_end_unixtime(
            epoch_slots_remaining,
            average_slot_time_seconds,
        ),
        data_sources,
        economics,
        node,
        mode_statuses,
        ..snapshot
    })
}
fn watched_account_balance_targets<'a>(
    vote_accounts: &'a VoteAccounts,
    matcher: &LabelMatcher,
) -> Vec<AccountBalanceTarget<'a>> {
    let mut targets = vote_accounts
        .current
        .iter()
        .chain(vote_accounts.delinquent.iter())
        .filter(|account| matcher.is_watched(&account.node_pubkey, &account.vote_pubkey))
        .map(|account| AccountBalanceTarget {
            identity: account.node_pubkey.as_str(),
            vote_account: account.vote_pubkey.as_str(),
        })
        .collect::<Vec<_>>();
    // `aggregate_records` uses the same key order for watched-validator rows.
    targets.sort_unstable_by(|left, right| {
        left.identity
            .cmp(right.identity)
            .then_with(|| left.vote_account.cmp(right.vote_account))
    });
    targets
}

fn include_economics_target<'a>(
    account_targets: &mut Vec<AccountBalanceTarget<'a>>,
    target: &'a EconomicsTarget,
) {
    if account_targets.iter().any(|candidate| {
        candidate.identity == target.identity && candidate.vote_account == target.vote_account
    }) {
        return;
    }
    account_targets.push(AccountBalanceTarget {
        identity: target.identity.as_str(),
        vote_account: target.vote_account.as_str(),
    });
}

fn economics_target_has_funding(
    target: &EconomicsTarget,
    account_targets: &[AccountBalanceTarget<'_>],
    balances: &[Option<AccountBalances>],
) -> bool {
    account_targets
        .iter()
        .zip(balances)
        .any(|(candidate, balances)| {
            candidate.identity == target.identity
                && candidate.vote_account == target.vote_account
                && balances.is_some()
        })
}

fn apply_gossip_observations(
    validators: &mut [WatchedValidatorMetrics],
    cluster_nodes: Vec<ClusterNode>,
) {
    for validator in &mut *validators {
        validator.gossip = GossipObservation::Missing;
    }
    // `aggregate_records` sorts by identity, so each matching range is contiguous.
    for node in cluster_nodes {
        let start = validators
            .partition_point(|validator| validator.identity.as_str() < node.pubkey.as_str());
        let count =
            validators[start..].partition_point(|validator| validator.identity == node.pubkey);
        if count == 0 {
            continue;
        }
        let observation = GossipObservation::Present {
            version: node.version,
            feature_set: node.feature_set,
            shred_version: node.shred_version,
        };
        if let Some((last, preceding)) = validators[start..start + count].split_last_mut() {
            for validator in preceding {
                validator.gossip = observation.clone();
            }
            last.gossip = observation;
        }
    }
}

fn recent_completed_leader_slots(
    relative_slots: &[u64],
    epoch_start_slot: u64,
    current_slot: u64,
) -> Vec<u64> {
    let mut latest_slots = BinaryHeap::with_capacity(MAX_RECENT_BLOCK_FEE_REWARD_SLOTS);
    for relative_slot in relative_slots {
        let Some(slot) = epoch_start_slot.checked_add(*relative_slot) else {
            continue;
        };
        if slot >= current_slot {
            continue;
        }

        let encoded_slot = u64::MAX - slot;
        if latest_slots
            .iter()
            .any(|existing| *existing == encoded_slot)
        {
            continue;
        }
        if latest_slots.len() < MAX_RECENT_BLOCK_FEE_REWARD_SLOTS {
            latest_slots.push(encoded_slot);
        } else if latest_slots
            .peek()
            .is_some_and(|oldest| encoded_slot < *oldest)
        {
            latest_slots.pop();
            latest_slots.push(encoded_slot);
        }
    }

    let mut slots = latest_slots.into_vec();
    for slot in &mut slots {
        *slot = u64::MAX - *slot;
    }
    slots.sort_unstable_by(|left, right| right.cmp(left));
    slots
}

#[cfg(test)]
mod tests {
    use super::{
        economics_target_has_funding, recent_completed_leader_slots,
        watched_account_balance_targets, AccountBalanceTarget, AccountBalances, EconomicsTarget,
    };
    use crate::{
        collector::rpc::{VoteAccountInfo, VoteAccounts},
        config::{Config, WatchedValidatorConfig},
        labels::LabelMatcher,
    };

    #[test]
    fn recent_completed_leader_slots_keeps_the_latest_bounded_unique_slots() {
        let mut relative_slots = (0..40).collect::<Vec<_>>();
        relative_slots.extend([39, u64::MAX]);

        let slots = recent_completed_leader_slots(&relative_slots, 100, 138);

        assert_eq!(slots, (106..=137).rev().collect::<Vec<_>>());
    }

    #[test]
    fn configured_target_funding_remains_required_when_other_accounts_are_absent() {
        let target = EconomicsTarget {
            identity: "target-id".to_string(),
            vote_account: "target-vote".to_string(),
        };
        let account_targets = [
            AccountBalanceTarget {
                identity: "missing-id",
                vote_account: "missing-vote",
            },
            AccountBalanceTarget {
                identity: "target-id",
                vote_account: "target-vote",
            },
        ];
        let target_balance = AccountBalances {
            identity_lamports: 1,
            vote_account_lamports: 2,
            vote_account_rent_exempt_minimum_lamports: Some(1),
        };

        assert!(economics_target_has_funding(
            &target,
            &account_targets,
            &[None, Some(target_balance)]
        ));
        assert!(!economics_target_has_funding(
            &target,
            &account_targets,
            &[None, None]
        ));
    }

    #[test]
    fn watched_account_balance_targets_follow_snapshot_order() {
        let config = Config {
            watched_validators: vec![
                WatchedValidatorConfig {
                    identity: Some("id-z".to_string()),
                    vote_account: Some("vote-z".to_string()),
                    labels: Vec::new(),
                },
                WatchedValidatorConfig {
                    identity: Some("id-a".to_string()),
                    vote_account: Some("vote-a".to_string()),
                    labels: Vec::new(),
                },
            ],
            ..Config::default()
        };
        let vote_account = |identity: &str, vote_account: &str| VoteAccountInfo {
            vote_pubkey: vote_account.to_string(),
            node_pubkey: identity.to_string(),
            activated_stake: 1,
            commission: 0,
            epoch_credits: Vec::new(),
            last_vote: 0,
            root_slot: 0,
        };
        let vote_accounts = VoteAccounts {
            current: vec![
                vote_account("id-z", "vote-z"),
                vote_account("id-a", "vote-a"),
            ],
            delinquent: Vec::new(),
        };
        let matcher = LabelMatcher::from_config(&config);

        let targets = watched_account_balance_targets(&vote_accounts, &matcher);

        assert_eq!(
            targets
                .iter()
                .map(|target| (target.identity, target.vote_account))
                .collect::<Vec<_>>(),
            vec![("id-a", "vote-a"), ("id-z", "vote-z")]
        );
    }
}
