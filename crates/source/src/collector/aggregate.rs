//! Pure conversion of decoded Solana records into operator-facing aggregates.
//!
//! Aggregation owns validator and scope calculations, economics derivation, and
//! epoch timing; it performs no transport or Prometheus I/O.

use super::{
    model::{
        AccountBalances, EconomicsMetrics, EpochCreditsSample, GossipObservation, RewardSample,
        ScopeMetrics, Snapshot, ValidatorFundingMetrics, ValidatorRecord, ValidatorStatus,
        WatchedValidatorMetrics, MAX_UPCOMING_LEADER_SLOTS,
    },
    rpc::{SlotRange, VoteAccountInfo, VoteAccounts},
};
use crate::{
    config::Config,
    labels::{LabelMatcher, LABEL_SCOPE_ALL_NETWORK, LABEL_SCOPE_NETWORK, LABEL_SCOPE_WATCHED},
};
use anyhow::{bail, Context, Result};
use std::{
    collections::{BTreeMap, BinaryHeap, HashMap},
    time::{SystemTime, UNIX_EPOCH},
};

const VAT_LAMPORTS_PER_EPOCH: u64 = 800_000_000;
#[derive(Default)]
struct ScopeAccumulator {
    current_count: u64,
    delinquent_count: u64,
    current_stake_lamports: u64,
    delinquent_stake_lamports: u64,
    latest_epoch_vote_credits: Option<u64>,
    latest_epoch_reward_lamports: u64,
    latest_epoch_reward_sample_count: u64,
    latest_epoch_rewarded_blocks_produced: u64,
    weighted_vote_efficiency_sum: f64,
    vote_efficiency_weight_sum: f64,
    weighted_last_vote_slot_lag_sum: f64,
    weighted_root_slot_lag_sum: f64,
    weighted_commission_sum: f64,
    weight_sum: f64,
    blocks_produced: u64,
    blocks_missed: u64,
    future_leader_slots: u64,
}

impl ScopeAccumulator {
    fn observe(
        &mut self,
        record: &ValidatorRecord,
        current_slot: u64,
        ideal_epoch_credits: Option<u64>,
        sample_epoch: Option<u64>,
        reward_sample_epoch: Option<u64>,
    ) {
        match record.status {
            ValidatorStatus::Current => {
                self.current_count += 1;
                self.current_stake_lamports += record.activated_stake_lamports;
            }
            ValidatorStatus::Delinquent => {
                self.delinquent_count += 1;
                self.delinquent_stake_lamports += record.activated_stake_lamports;
            }
        }

        let weight = record.activated_stake_lamports.max(1) as f64;
        self.weight_sum += weight;
        self.weighted_last_vote_slot_lag_sum +=
            weight * record.last_vote_slot_lag(current_slot) as f64;
        self.weighted_root_slot_lag_sum += weight * record.root_slot_lag(current_slot) as f64;
        self.weighted_commission_sum += weight * f64::from(record.commission);

        if let Some(credits) = record.matching_epoch_credits(sample_epoch) {
            self.latest_epoch_vote_credits = Some(
                self.latest_epoch_vote_credits
                    .unwrap_or_default()
                    .saturating_add(credits),
            );
        }
        if let Some(vote_efficiency_pct) =
            record.vote_efficiency_pct(ideal_epoch_credits, sample_epoch)
        {
            self.weighted_vote_efficiency_sum += weight * vote_efficiency_pct;
            self.vote_efficiency_weight_sum += weight;
        }

        self.latest_epoch_rewarded_blocks_produced += record.latest_epoch_rewarded_blocks_produced;
        if let Some(reward_lamports) = record.matching_epoch_reward_lamports(reward_sample_epoch) {
            self.latest_epoch_reward_lamports += reward_lamports;
            self.latest_epoch_reward_sample_count += 1;
        }

        self.blocks_produced += record.current_epoch_blocks_produced;
        self.blocks_missed += record.blocks_missed();
        self.future_leader_slots += record.future_leader_slots;
    }

    fn finish(self) -> ScopeMetrics {
        let weight_sum = if self.weight_sum > 0.0 {
            self.weight_sum
        } else {
            1.0
        };
        let total_blocks = self.blocks_produced + self.blocks_missed;
        let validator_count = self.current_count.saturating_add(self.delinquent_count);
        let latest_epoch_reward_lamports = (validator_count > 0
            && self.latest_epoch_reward_sample_count == validator_count)
            .then_some(self.latest_epoch_reward_lamports);

        ScopeMetrics {
            current_count: self.current_count,
            delinquent_count: self.delinquent_count,
            current_stake_lamports: self.current_stake_lamports,
            delinquent_stake_lamports: self.delinquent_stake_lamports,
            latest_epoch_vote_credits: self.latest_epoch_vote_credits,
            latest_epoch_reward_lamports,
            latest_epoch_rewarded_blocks_produced: self.latest_epoch_rewarded_blocks_produced,
            vote_efficiency_pct: (self.vote_efficiency_weight_sum > 0.0)
                .then(|| self.weighted_vote_efficiency_sum / self.vote_efficiency_weight_sum),
            last_vote_slot_lag: self.weighted_last_vote_slot_lag_sum / weight_sum,
            root_slot_lag: self.weighted_root_slot_lag_sum / weight_sum,
            average_commission_pct: self.weighted_commission_sum / weight_sum,
            blocks_produced: self.blocks_produced,
            blocks_missed: self.blocks_missed,
            block_production_rate: ratio(self.blocks_produced, total_blocks),
            future_leader_slots: self.future_leader_slots,
        }
    }
}
/// Shared immutable inputs used to normalize one vote account.
pub(super) struct RecordBuildContext<'a> {
    pub(super) matcher: &'a LabelMatcher,
    pub(super) current_epoch: u64,
    pub(super) current_slot: u64,
    pub(super) epoch_start_slot: u64,
    pub(super) next_epoch_start_slot: u64,
    pub(super) average_slot_time_seconds: Option<f64>,
    pub(super) by_identity: &'a HashMap<String, (u64, u64)>,
    pub(super) reward_epoch_by_identity: &'a HashMap<String, (u64, u64)>,
    pub(super) current_schedule: Option<&'a HashMap<String, Vec<u64>>>,
    pub(super) next_schedule: Option<&'a HashMap<String, Vec<u64>>>,
    pub(super) latest_epoch_rewards: &'a HashMap<String, RewardSample>,
}

/// Normalizes one decoded vote account with leadership, reward, and scope data.
pub(super) fn build_record(
    info: VoteAccountInfo,
    status: ValidatorStatus,
    context: &RecordBuildContext<'_>,
) -> ValidatorRecord {
    let (leader_slots, blocks_produced) = context
        .by_identity
        .get(&info.node_pubkey)
        .copied()
        .unwrap_or((0, 0));
    let reward_epoch_blocks_produced = context
        .reward_epoch_by_identity
        .get(&info.node_pubkey)
        .copied()
        .map_or(0, |(_, produced)| produced);
    let scopes = context
        .matcher
        .labels_for(&info.node_pubkey, &info.vote_pubkey);

    let current_epoch_schedule = context.current_schedule.map(|schedule| {
        schedule
            .get(&info.node_pubkey)
            .map_or(&[][..], Vec::as_slice)
    });
    let next_epoch_schedule = context.next_schedule.map(|schedule| {
        schedule
            .get(&info.node_pubkey)
            .map_or(&[][..], Vec::as_slice)
    });
    let current_epoch_scheduled_leader_slots =
        current_epoch_schedule.map(|slots| u64::try_from(slots.len()).unwrap_or(u64::MAX));
    let current_epoch_future_slots = current_epoch_schedule
        .into_iter()
        .flatten()
        .filter_map(|relative_slot| context.epoch_start_slot.checked_add(*relative_slot))
        .filter(|absolute_slot| *absolute_slot >= context.current_slot);
    let next_epoch_future_slots = next_epoch_schedule
        .into_iter()
        .flatten()
        .filter_map(|relative_slot| context.next_epoch_start_slot.checked_add(*relative_slot));
    let (future_leader_slots, next_leader_slot) = current_epoch_future_slots
        .chain(next_epoch_future_slots)
        .fold((0_u64, None), |(count, next), slot| {
            (
                count.saturating_add(1),
                Some(next.map_or(slot, |current: u64| current.min(slot))),
            )
        });
    let slots_until_next_leader =
        next_leader_slot.map(|slot| slot.saturating_sub(context.current_slot));
    let seconds_until_next_leader = slots_until_next_leader
        .zip(context.average_slot_time_seconds)
        .map(|(slots, slot_time)| slots as f64 * slot_time);
    let upcoming_leader_slots = if scopes.iter().any(|scope| scope == LABEL_SCOPE_WATCHED) {
        bounded_upcoming_leader_slots(
            current_epoch_schedule,
            next_epoch_schedule,
            context.epoch_start_slot,
            context.next_epoch_start_slot,
            context.current_slot,
        )
    } else {
        Vec::new()
    };

    let latest_epoch_credits = info
        .epoch_credits
        .iter()
        .filter(|(epoch, _, _)| *epoch <= context.current_epoch)
        .max_by_key(|(epoch, _, _)| *epoch)
        .map(|(epoch, credits, previous)| EpochCreditsSample {
            epoch: *epoch,
            earned: credits.saturating_sub(*previous),
        });
    let latest_epoch_reward = context.latest_epoch_rewards.get(&info.vote_pubkey).cloned();

    ValidatorRecord {
        identity: info.node_pubkey,
        vote_account: info.vote_pubkey,
        status,
        activated_stake_lamports: info.activated_stake,
        commission: info.commission,
        last_vote: info.last_vote,
        root_slot: info.root_slot,
        latest_epoch_credits,
        latest_epoch_reward,
        latest_epoch_rewarded_blocks_produced: reward_epoch_blocks_produced,
        current_epoch_leader_slots: leader_slots,
        current_epoch_blocks_produced: blocks_produced,
        current_epoch_scheduled_leader_slots,
        future_leader_slots,
        next_leader_slot,
        slots_until_next_leader,
        seconds_until_next_leader,
        upcoming_leader_slots,
        scopes,
    }
}

fn bounded_upcoming_leader_slots(
    current_epoch_schedule: Option<&[u64]>,
    next_epoch_schedule: Option<&[u64]>,
    epoch_start_slot: u64,
    next_epoch_start_slot: u64,
    current_slot: u64,
) -> Vec<u64> {
    let current_slots = current_epoch_schedule
        .into_iter()
        .flatten()
        .filter_map(|relative_slot| epoch_start_slot.checked_add(*relative_slot))
        .filter(|absolute_slot| *absolute_slot >= current_slot);
    let next_slots = next_epoch_schedule
        .into_iter()
        .flatten()
        .filter_map(|relative_slot| next_epoch_start_slot.checked_add(*relative_slot));
    let mut upcoming = BinaryHeap::with_capacity(MAX_UPCOMING_LEADER_SLOTS);

    for slot in current_slots.chain(next_slots) {
        if upcoming.len() < MAX_UPCOMING_LEADER_SLOTS {
            upcoming.push(slot);
        } else if upcoming.peek().is_some_and(|latest| slot < *latest) {
            upcoming.pop();
            upcoming.push(slot);
        }
    }

    upcoming.into_sorted_vec()
}

/// Aggregates normalized validator records into deterministic scope and detail rows.
pub(super) fn aggregate_records(
    network: String,
    epoch: u64,
    slot: u64,
    records: &[ValidatorRecord],
) -> Snapshot {
    let sample_epoch = records
        .iter()
        .filter_map(|record| {
            record
                .latest_epoch_credits
                .as_ref()
                .map(|credits| credits.epoch)
        })
        .filter(|sample_epoch| *sample_epoch <= epoch)
        .max();
    let reward_sample_epoch = records
        .iter()
        .filter_map(|record| {
            record
                .latest_epoch_reward
                .as_ref()
                .map(|reward| reward.epoch)
        })
        .max();

    let ideal_epoch_credits = sample_epoch.and_then(|expected_epoch| {
        records
            .iter()
            .filter_map(|record| record.latest_epoch_credits.as_ref())
            .filter(|credits| credits.epoch == expected_epoch)
            .map(|credits| credits.earned)
            .max()
    });

    let mut scopes: BTreeMap<String, ScopeAccumulator> = BTreeMap::new();
    scopes
        .entry(LABEL_SCOPE_ALL_NETWORK.to_string())
        .or_default();
    scopes.entry(LABEL_SCOPE_NETWORK.to_string()).or_default();
    scopes.entry(LABEL_SCOPE_WATCHED.to_string()).or_default();

    for record in records {
        for scope in &record.scopes {
            scopes.entry(scope.clone()).or_default().observe(
                record,
                slot,
                ideal_epoch_credits,
                sample_epoch,
                reward_sample_epoch,
            );
        }
    }

    let mut watched_validators = records
        .iter()
        .filter(|record| record.is_watched())
        .map(|record| {
            build_watched_validator_metrics(
                record,
                slot,
                ideal_epoch_credits,
                sample_epoch,
                reward_sample_epoch,
            )
        })
        .collect::<Vec<_>>();
    watched_validators.sort_by(|left, right| {
        left.identity
            .cmp(&right.identity)
            .then_with(|| left.vote_account.cmp(&right.vote_account))
    });

    Snapshot {
        network,
        epoch,
        slot,
        epoch_slots: 0,
        epoch_progress_pct: 0.0,
        epoch_slots_remaining: 0,
        cluster_performance: None,
        estimated_epoch_end_unixtime: None,
        vote_credits_sample_epoch: sample_epoch,
        reward_sample_epoch,
        scopes: scopes
            .into_iter()
            .map(|(scope, metrics)| (scope, metrics.finish()))
            .collect(),
        watched_validators,
        data_sources: vec![],
        mode_statuses: vec![],
        node: None,
        economics: None,
    }
}

/// Projects a watched validator record into its exported metric values.
pub(super) fn build_watched_validator_metrics(
    record: &ValidatorRecord,
    current_slot: u64,
    ideal_epoch_credits: Option<u64>,
    sample_epoch: Option<u64>,
    reward_sample_epoch: Option<u64>,
) -> WatchedValidatorMetrics {
    let latest_epoch_vote_credits = record.matching_epoch_credits(sample_epoch);
    let latest_epoch_reward_lamports = record.matching_epoch_reward_lamports(reward_sample_epoch);
    let vote_efficiency_pct = record.vote_efficiency_pct(ideal_epoch_credits, sample_epoch);

    WatchedValidatorMetrics {
        identity: record.identity.clone(),
        vote_account: record.vote_account.clone(),
        status: record.status,
        gossip: GossipObservation::Unavailable,
        scopes: record.scopes.clone(),
        activated_stake_lamports: record.activated_stake_lamports,
        funding: None,
        latest_epoch_vote_credits,
        vote_efficiency_pct,
        last_vote_slot_lag: record.last_vote_slot_lag(current_slot),
        root_slot_lag: record.root_slot_lag(current_slot),
        commission_pct: record.commission,
        latest_epoch_reward_lamports,
        latest_epoch_rewarded_blocks_produced: record.latest_epoch_rewarded_blocks_produced,
        blocks_produced: record.current_epoch_blocks_produced,
        blocks_missed: record.blocks_missed(),
        block_production_rate: ratio(
            record.current_epoch_blocks_produced,
            record.current_epoch_leader_slots,
        ),
        current_epoch_scheduled_leader_slots: record.current_epoch_scheduled_leader_slots,
        future_leader_slots: record.future_leader_slots,
        next_leader_slot: record.next_leader_slot,
        slots_until_next_leader: record.slots_until_next_leader,
        seconds_until_next_leader: record.seconds_until_next_leader,
        upcoming_leader_slots: record.upcoming_leader_slots.clone(),
    }
}

/// Returns the complete prior-epoch slot range when it is retained.
pub(super) fn reward_epoch_block_range(
    epoch_start_slot: u64,
    slots_in_epoch: u64,
    minimum_ledger_slot: u64,
) -> Option<SlotRange> {
    if epoch_start_slot == 0 || slots_in_epoch == 0 {
        return None;
    }

    let first_slot = epoch_start_slot.checked_sub(slots_in_epoch)?;
    let last_slot = epoch_start_slot.checked_sub(1)?;

    if first_slot < minimum_ledger_slot {
        return None;
    }

    Some(SlotRange {
        first_slot,
        last_slot,
    })
}

/// Resolved identity and vote-account pair used by economics collection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct EconomicsTarget {
    /// Validator identity account address.
    pub(super) identity: String,
    /// Validator vote-account address.
    pub(super) vote_account: String,
}

/// Resolves the configured economics selector against live vote accounts.
pub(super) fn resolve_economics_target(
    config: &Config,
    vote_accounts: &VoteAccounts,
) -> Result<EconomicsTarget> {
    let configured = config
        .modes
        .economics
        .validator
        .as_ref()
        .context("economics mode requires a configured validator")?;
    let target = vote_accounts
        .current
        .iter()
        .chain(vote_accounts.delinquent.iter())
        .find(|info| {
            configured
                .identity
                .as_ref()
                .is_none_or(|identity| identity == &info.node_pubkey)
                && configured
                    .vote_account
                    .as_ref()
                    .is_none_or(|vote_account| vote_account == &info.vote_pubkey)
        })
        .context("configured economics validator is not present in getVoteAccounts")?;

    Ok(EconomicsTarget {
        identity: target.node_pubkey.clone(),
        vote_account: target.vote_pubkey.clone(),
    })
}

/// Derives vote-account VAT reserve and 0.8 SOL runway values from current balances.
pub(super) fn build_validator_funding_metrics(
    balances: AccountBalances,
) -> ValidatorFundingMetrics {
    let vat_reserve_lamports = balances
        .vote_account_rent_exempt_minimum_lamports
        .map(|minimum| balances.vote_account_lamports.saturating_sub(minimum));

    ValidatorFundingMetrics {
        identity_balance_lamports: balances.identity_lamports,
        vote_account_balance_lamports: balances.vote_account_lamports,
        vote_account_rent_exempt_minimum_lamports: balances
            .vote_account_rent_exempt_minimum_lamports,
        vat_reserve_lamports,
        vat_epochs_available: vat_reserve_lamports.map(|reserve| reserve / VAT_LAMPORTS_PER_EPOCH),
        vat_next_epoch_funded: vat_reserve_lamports
            .map(|reserve| reserve >= VAT_LAMPORTS_PER_EPOCH),
    }
}

/// Attaches one ordered funding result to each watched validator.
pub(super) fn apply_validator_funding_metrics(
    validators: &mut [WatchedValidatorMetrics],
    balances: Vec<Option<AccountBalances>>,
) -> Result<()> {
    if validators.len() != balances.len() {
        bail!(
            "account balance collection returned {} validators for {} watched validators",
            balances.len(),
            validators.len()
        );
    }
    for (validator, balances) in validators.iter_mut().zip(balances) {
        validator.funding = balances.map(build_validator_funding_metrics);
    }
    Ok(())
}

/// Combines configured-target funding, provider, and block-fee values.
pub(super) fn build_economics_metrics(
    target: &EconomicsTarget,
    balances: AccountBalances,
    jito_mev_reward: Option<super::JitoMevReward>,
    trillium_mev_capture: Option<super::TrilliumMevCapture>,
    recent_block_fee_rewards: Option<super::RecentBlockFeeRewards>,
) -> EconomicsMetrics {
    let funding = build_validator_funding_metrics(balances);
    EconomicsMetrics {
        identity: target.identity.clone(),
        vote_account: target.vote_account.clone(),
        identity_balance_lamports: funding.identity_balance_lamports,
        vote_account_balance_lamports: funding.vote_account_balance_lamports,
        vote_account_rent_exempt_minimum_lamports: funding
            .vote_account_rent_exempt_minimum_lamports,
        vat_reserve_lamports: funding.vat_reserve_lamports,
        vat_epochs_available: funding.vat_epochs_available,
        vat_next_epoch_funded: funding.vat_next_epoch_funded,
        jito_mev_reward,
        trillium_mev_capture,
        recent_block_fee_rewards,
    }
}

/// Estimates epoch completion from the current clock and average slot duration.
pub(super) fn estimated_epoch_end_unixtime(
    slots_remaining: u64,
    average_slot_time_seconds: Option<f64>,
) -> Option<f64> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs_f64();
    estimated_epoch_end_unixtime_at(slots_remaining, average_slot_time_seconds, now)
}

/// Estimates epoch completion relative to an explicit Unix timestamp.
pub(super) fn estimated_epoch_end_unixtime_at(
    slots_remaining: u64,
    average_slot_time_seconds: Option<f64>,
    now_unixtime: f64,
) -> Option<f64> {
    average_slot_time_seconds.map(|slot_time| now_unixtime + slots_remaining as f64 * slot_time)
}

/// Returns a percentage, using zero for a zero denominator.
pub(super) fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        return 0.0;
    }
    (numerator as f64 / denominator as f64) * 100.0
}
