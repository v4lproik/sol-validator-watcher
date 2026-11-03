use super::aggregate::ratio;
use crate::{
    labels::LABEL_SCOPE_WATCHED,
    mode::{CommandId, ModeId, ProviderFeature, SolanaRpcCompatibility, Transport},
    source::SourceSelection,
};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidatorStatus {
    Current,
    Delinquent,
}

impl ValidatorStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Delinquent => "delinquent",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EpochCreditsSample {
    pub epoch: u64,
    pub earned: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RewardSample {
    pub epoch: u64,
    pub amount_lamports: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AccountBalances {
    pub identity_lamports: u64,
    pub vote_account_lamports: u64,
    pub vote_account_rent_exempt_minimum_lamports: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EconomicsMetrics {
    pub identity: String,
    pub vote_account: String,
    pub identity_balance_lamports: u64,
    pub vote_account_balance_lamports: u64,
    pub vote_account_rent_exempt_minimum_lamports: Option<u64>,
    pub vat_reserve_lamports: Option<u64>,
    pub vat_epochs_available: Option<u64>,
    pub vat_next_epoch_funded: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WatchedValidatorMetrics {
    pub identity: String,
    pub vote_account: String,
    pub status: ValidatorStatus,
    pub scopes: Vec<String>,
    pub activated_stake_lamports: u64,
    pub latest_epoch_vote_credits: Option<u64>,
    pub vote_efficiency_pct: Option<f64>,
    pub last_vote_slot_lag: u64,
    pub root_slot_lag: u64,
    pub commission_pct: u8,
    pub latest_epoch_reward_lamports: u64,
    pub latest_epoch_rewarded_blocks_produced: u64,
    pub blocks_produced: u64,
    pub blocks_missed: u64,
    pub block_production_rate: f64,
    pub future_leader_slots: u64,
    pub next_leader_slot: Option<u64>,
    pub slots_until_next_leader: Option<u64>,
    pub seconds_until_next_leader: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValidatorRecord {
    pub identity: String,
    pub vote_account: String,
    pub status: ValidatorStatus,
    pub activated_stake_lamports: u64,
    pub commission: u8,
    pub last_vote: u64,
    pub root_slot: u64,
    pub latest_epoch_credits: Option<EpochCreditsSample>,
    pub latest_epoch_reward: Option<RewardSample>,
    pub latest_epoch_rewarded_blocks_produced: u64,
    pub current_epoch_leader_slots: u64,
    pub current_epoch_blocks_produced: u64,
    pub future_leader_slots: u64,
    pub next_leader_slot: Option<u64>,
    pub slots_until_next_leader: Option<u64>,
    pub seconds_until_next_leader: Option<f64>,
    pub scopes: Vec<String>,
}

impl ValidatorRecord {
    pub(super) fn is_watched(&self) -> bool {
        self.scopes.iter().any(|scope| scope == LABEL_SCOPE_WATCHED)
    }

    pub(super) fn last_vote_slot_lag(&self, current_slot: u64) -> u64 {
        current_slot.saturating_sub(self.last_vote)
    }

    pub(super) fn root_slot_lag(&self, current_slot: u64) -> u64 {
        current_slot.saturating_sub(self.root_slot)
    }

    pub(super) fn matching_epoch_credits(&self, sample_epoch: Option<u64>) -> Option<u64> {
        self.latest_epoch_credits
            .as_ref()
            .filter(|credits| Some(credits.epoch) == sample_epoch)
            .map(|credits| credits.earned)
    }

    pub(super) fn matching_epoch_reward_lamports(
        &self,
        reward_sample_epoch: Option<u64>,
    ) -> Option<u64> {
        self.latest_epoch_reward
            .as_ref()
            .filter(|reward| Some(reward.epoch) == reward_sample_epoch)
            .map(|reward| reward.amount_lamports)
    }

    pub(super) fn vote_efficiency_pct(
        &self,
        ideal_epoch_credits: Option<u64>,
        sample_epoch: Option<u64>,
    ) -> Option<f64> {
        let credits = self.matching_epoch_credits(sample_epoch)?;
        let ideal = ideal_epoch_credits.filter(|ideal| *ideal > 0)?;

        Some(ratio(credits, ideal))
    }

    pub(super) fn blocks_missed(&self) -> u64 {
        self.current_epoch_leader_slots
            .saturating_sub(self.current_epoch_blocks_produced)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScopeMetrics {
    pub current_count: u64,
    pub delinquent_count: u64,
    pub current_stake_lamports: u64,
    pub delinquent_stake_lamports: u64,
    pub latest_epoch_vote_credits: Option<u64>,
    pub latest_epoch_reward_lamports: u64,
    pub latest_epoch_rewarded_blocks_produced: u64,
    pub vote_efficiency_pct: Option<f64>,
    pub last_vote_slot_lag: f64,
    pub root_slot_lag: f64,
    pub average_commission_pct: f64,
    pub blocks_produced: u64,
    pub blocks_missed: u64,
    pub block_production_rate: f64,
    pub future_leader_slots: u64,
}

impl ScopeMetrics {
    pub(crate) fn activated_stake_lamports(&self) -> u64 {
        self.current_stake_lamports + self.delinquent_stake_lamports
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub network: String,
    pub epoch: u64,
    pub slot: u64,
    pub epoch_progress_pct: f64,
    pub epoch_slots_remaining: u64,
    pub average_slot_time_seconds: Option<f64>,
    pub estimated_epoch_end_unixtime: Option<f64>,
    pub vote_credits_sample_epoch: Option<u64>,
    pub reward_sample_epoch: Option<u64>,
    pub scopes: BTreeMap<String, ScopeMetrics>,
    pub watched_validators: Vec<WatchedValidatorMetrics>,
    pub data_sources: Vec<SnapshotDataSource>,
    pub mode_statuses: Vec<ModeStatus>,
    pub economics: Option<EconomicsMetrics>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModeStatus {
    pub mode: ModeId,
    pub up: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotDataSource {
    pub mode: ModeId,
    pub command: CommandId,
    pub connector: Arc<str>,
    pub provider: Arc<str>,
    pub transport: Transport,
    pub solana_rpc_compatibility: Option<SolanaRpcCompatibility>,
    pub feature: Option<ProviderFeature>,
    pub precedence: usize,
}

impl From<SourceSelection> for SnapshotDataSource {
    fn from(source: SourceSelection) -> Self {
        Self {
            mode: source.mode,
            command: source.command,
            connector: source.connector,
            provider: source.provider,
            transport: source.transport,
            solana_rpc_compatibility: source.solana_rpc_compatibility,
            feature: source.feature,
            precedence: source.precedence,
        }
    }
}
