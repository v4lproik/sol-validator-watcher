//! Transactional orchestration of one collected watcher snapshot.
//!
//! Each command resolves through its own source route. Provenance is recorded
//! only after a command succeeds, and optional absence remains explicit.

use super::{
    aggregate::{
        aggregate_records, build_economics_metrics, build_record, estimated_epoch_end_unixtime,
        ratio, resolve_economics_target, RecordBuildContext,
    },
    command,
    model::{ModeStatus, NodeVotingStatus, Snapshot, ValidatorStatus},
    rpc::BlockProduction,
};
use crate::{config::Config, labels::LabelMatcher, mode::ModeId, source::SourceChain};
use anyhow::Result;

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
    sources
        .required(&command::GetAccountBalances {
            identity: &target.identity,
            vote_account: &target.vote_account,
        })
        .await?;

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
    let (epoch_info, vote_accounts, recent_performance, node_status) = tokio::join!(
        sources.required(&epoch_command),
        sources.required(&vote_accounts_command),
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
    let average_slot_time_seconds = recent_performance.and_then(|resolved| {
        data_sources.push(resolved.source.into());
        resolved.value
    });

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
                return Ok::<_, anyhow::Error>((None, None, None));
            };
            let balances = sources
                .required(&command::GetAccountBalances {
                    identity: &target.identity,
                    vote_account: &target.vote_account,
                })
                .await?;

            let (Some(epoch), Some(addresses)) = (reward_epoch, reward_vote_accounts.as_deref())
            else {
                return Ok((Some(balances), None, None));
            };
            let inflation_rewards_command = command::GetInflationRewards { addresses, epoch };
            let block_production_command = command::GetRewardEpochBlockProduction {
                epoch_start_slot,
                slots_in_epoch: epoch_info.value.slots_in_epoch,
            };
            let (inflation_rewards, block_production) = tokio::join!(
                sources.required(&inflation_rewards_command),
                sources.required(&block_production_command),
            );
            Ok((
                Some(balances),
                Some(inflation_rewards?),
                Some(block_production?),
            ))
        },
    );

    let (account_balances, inflation_rewards, reward_epoch_block_production) = economics_outputs?;
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
    let economics = match (economics_target.as_ref(), account_balances) {
        (Some(target), Some(resolved)) => {
            data_sources.push(resolved.source.into());
            Some(build_economics_metrics(target, resolved.value))
        }
        _ => None,
    };
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
    let mut records = Vec::with_capacity(
        vote_accounts.value.current.len() + vote_accounts.value.delinquent.len(),
    );

    for info in vote_accounts.value.current {
        records.push(build_record(info, ValidatorStatus::Current, &build_context));
    }
    for info in vote_accounts.value.delinquent {
        records.push(build_record(
            info,
            ValidatorStatus::Delinquent,
            &build_context,
        ));
    }

    let mut snapshot = aggregate_records(
        config.network.clone(),
        epoch_info.value.epoch,
        current_slot,
        &records,
    );
    snapshot.epoch_progress_pct =
        ratio(epoch_info.value.slot_index, epoch_info.value.slots_in_epoch);
    snapshot.epoch_slots_remaining = epoch_info
        .value
        .slots_in_epoch
        .saturating_sub(epoch_info.value.slot_index);
    snapshot.average_slot_time_seconds = average_slot_time_seconds;
    snapshot.estimated_epoch_end_unixtime =
        estimated_epoch_end_unixtime(snapshot.epoch_slots_remaining, average_slot_time_seconds);
    snapshot.data_sources = data_sources;
    snapshot.economics = economics;
    snapshot.node = node;
    snapshot.mode_statuses = vec![
        ModeStatus {
            mode: ModeId::Consensus,
            up: true,
        },
        ModeStatus {
            mode: ModeId::Leadership,
            up: leadership_up,
        },
    ];
    if economics_enabled {
        snapshot.mode_statuses.push(ModeStatus {
            mode: ModeId::Economics,
            up: true,
        });
    }
    Ok(snapshot)
}
