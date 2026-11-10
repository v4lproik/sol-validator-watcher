//! Collector domain records and completed snapshot models.
//!
//! These types carry bounded collection results into aggregation and Prometheus
//! publication without coupling collection code to metric collectors.

use crate::{
    CommandId, ModeId, ProviderFeature, SolanaRpcCompatibility, SourceSelection, Transport,
    LABEL_SCOPE_WATCHED,
};
use std::{collections::BTreeMap, sync::Arc};

/// Maximum upcoming assignments exported per watched validator.
pub const MAX_UPCOMING_LEADER_SLOTS: usize = 16;

/// Cluster-reported voting state for a validator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidatorStatus {
    /// The vote account is participating in the current set.
    Current,
    /// The vote account is not keeping pace with cluster voting.
    Delinquent,
}

impl ValidatorStatus {
    /// Returns the stable Prometheus label value.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Delinquent => "delinquent",
        }
    }
}

/// Health reported by one node-local RPC endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeHealth {
    /// The endpoint is within its configured distance of the cluster tip.
    Healthy,
    /// The endpoint reports that it is behind the cluster tip.
    Behind {
        /// Reported slot distance, when the unstable Solana error payload includes it.
        slots: Option<u64>,
    },
}

impl NodeHealth {
    /// Reports whether the node considers itself healthy.
    pub const fn is_healthy(self) -> bool {
        matches!(self, Self::Healthy)
    }

    /// Returns zero for a healthy node or the reported lag for an unhealthy node.
    pub const fn catchup_distance_slots(self) -> Option<u64> {
        match self {
            Self::Healthy => Some(0),
            Self::Behind { slots } => slots,
        }
    }
}

/// Cluster-observed vote-account state for a node identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeVotingStatus {
    /// At least one current vote account names the node identity.
    Current,
    /// No current vote account and at least one delinquent vote account names the identity.
    Delinquent,
    /// No current or delinquent vote account names the identity.
    NotFound,
}

impl NodeVotingStatus {
    /// Returns the stable Prometheus label value.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Delinquent => "delinquent",
            Self::NotFound => "not-found",
        }
    }
}

/// Node-local health and software values published from one coherent RPC source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeMetrics {
    /// Active node identity returned by `getIdentity`.
    pub identity: String,
    /// Validator client version returned by `getVersion`.
    pub version: String,
    /// Validator feature-set identifier when reported by the node.
    pub feature_set: Option<u32>,
    /// Block height reported by the node.
    pub block_height: u64,
    /// Node health relative to the cluster tip.
    pub health: NodeHealth,
    /// Vote-account state observed in the same watcher snapshot.
    pub voting_status: NodeVotingStatus,
}

/// Node-local values collected before vote-account state is joined into the snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeRpcStatus {
    pub identity: String,
    pub version: String,
    pub feature_set: Option<u32>,
    pub block_height: u64,
    pub health: NodeHealth,
}

impl NodeRpcStatus {
    pub fn with_voting_status(self, voting_status: NodeVotingStatus) -> NodeMetrics {
        NodeMetrics {
            identity: self.identity,
            version: self.version,
            feature_set: self.feature_set,
            block_height: self.block_height,
            health: self.health,
            voting_status,
        }
    }
}

/// Vote credits earned in one epoch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EpochCreditsSample {
    /// Epoch containing the sample.
    pub epoch: u64,
    /// Credits earned during that epoch.
    pub earned: u64,
}

/// Inflation reward earned in one epoch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RewardSample {
    /// Epoch in which the reward was paid.
    pub epoch: u64,
    /// Reward amount in lamports.
    pub amount_lamports: u64,
}
/// Settled Jito MEV reward for one validator and epoch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JitoMevReward {
    /// Epoch in which the reward was earned.
    pub epoch: u64,
    /// Post-commission MEV lamports paid to the validator and its stakers.
    pub reward_lamports: u64,
    /// Validator MEV commission in basis points.
    pub commission_bps: u16,
}
/// Completed-epoch block-reward economics published by Trillium.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TrilliumBlockRewards {
    /// Validator block rewards after protocol burn, in SOL.
    pub after_burn_sol: Option<f64>,
    /// Total priority fees paid by produced blocks, in SOL.
    pub priority_fees_sol: Option<f64>,
    /// Priority fees allocated to the validator, in SOL.
    pub validator_priority_fees_sol: Option<f64>,
    /// Priority fees allocated to delegators, in SOL.
    pub delegator_priority_fees_sol: Option<f64>,
    /// Signature fees allocated to the validator, in SOL.
    pub validator_signature_fees_sol: Option<f64>,
    /// Average MEV captured per produced block, in SOL.
    pub average_mev_per_block_sol: Option<f64>,
    /// Median MEV captured per produced block, in SOL.
    pub median_mev_per_block_sol: Option<f64>,
    /// Average priority fees per produced block, in SOL.
    pub average_priority_fees_per_block_sol: Option<f64>,
    /// Median priority fees per produced block, in SOL.
    pub median_priority_fees_per_block_sol: Option<f64>,
    /// Average signature fees per produced block, in SOL.
    pub average_signature_fees_per_block_sol: Option<f64>,
    /// Median signature fees per produced block, in SOL.
    pub median_signature_fees_per_block_sol: Option<f64>,
    /// Average gross Jito tips per produced block, in SOL.
    pub average_jito_tips_per_block_sol: Option<f64>,
    /// Median gross Jito tips per produced block, in SOL.
    pub median_jito_tips_per_block_sol: Option<f64>,
    /// Validator priority-fee commission percentage.
    pub priority_fee_commission_pct: Option<f64>,
}

/// Completed-epoch transaction and compute throughput published by Trillium.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TrilliumTransactionMetrics {
    /// Successful transactions included in produced blocks.
    pub successful: Option<u64>,
    /// Failed transactions included in produced blocks.
    pub failed: Option<u64>,
    /// Successful user transactions included in produced blocks.
    pub successful_user: Option<u64>,
    /// Failed user transactions included in produced blocks.
    pub failed_user: Option<u64>,
    /// Average transactions per produced block.
    pub average_per_block: Option<f64>,
    /// Average user transactions per produced block.
    pub average_user_per_block: Option<f64>,
    /// Average vote transactions per produced block.
    pub average_vote_per_block: Option<f64>,
    /// Median transactions per produced block.
    pub median_per_block: Option<f64>,
    /// Median user transactions per produced block.
    pub median_user_per_block: Option<f64>,
    /// Average transaction success percentage.
    pub average_success_pct: Option<f64>,
    /// Average user-transaction success percentage.
    pub average_user_success_pct: Option<f64>,
    /// Average compute units consumed per produced block.
    pub average_compute_units_per_block: Option<f64>,
    /// Median compute units consumed per produced block.
    pub median_compute_units_per_block: Option<f64>,
}

/// Latest completed-epoch validator economics published by Trillium.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrilliumMevCapture {
    /// Completed epoch represented by these values.
    pub epoch: u64,
    /// Total MEV captured by the validator's blocks, in SOL.
    pub captured_sol: Option<f64>,
    /// MEV allocated to the validator, in SOL.
    pub validator_sol: Option<f64>,
    /// MEV allocated to delegators, in SOL.
    pub stakers_sol: Option<f64>,
    /// Jito block-engine fee, in SOL.
    pub block_engine_sol: Option<f64>,
    /// Jito tip-router fee, in SOL.
    pub tip_router_sol: Option<f64>,
    /// MEV captured per assigned leader slot, in SOL.
    pub per_leader_slot_sol: Option<f64>,
    /// Validator MEV commission percentage.
    pub commission_pct: Option<f64>,
    /// Gross Jito tips deposited for the epoch, in SOL.
    pub total_jito_tips_sol: Option<f64>,
    /// Jito tip transactions observed for the epoch.
    pub total_jito_tip_count: Option<u64>,
    /// Assigned leader slots in the epoch.
    pub leader_slots: u64,
    /// Blocks produced in the epoch.
    pub blocks_produced: u64,
    /// Activated stake represented by the epoch, in SOL.
    pub activated_stake_sol: Option<f64>,
    /// Block-reward components and per-block reward distributions.
    pub block_rewards: TrilliumBlockRewards,
    /// Transaction outcomes, per-block throughput, and compute consumption.
    pub transactions: TrilliumTransactionMetrics,
}
/// Validator fee rewards sampled from recent confirmed blocks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecentBlockFeeRewards {
    /// Most recent produced slot in the bounded sample.
    pub latest_slot: u64,
    /// Validator fee reward paid for the most recent sampled block.
    pub latest_reward_lamports: u64,
    /// Sum of validator fee rewards across sampled produced blocks.
    pub total_reward_lamports: u64,
    /// Number of produced blocks represented by the sample.
    pub sampled_block_count: u64,
}

/// Validator addresses requested together for account-funding collection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountBalanceTarget<'a> {
    /// Validator identity account address.
    pub identity: &'a str,
    /// Validator vote-account address.
    pub vote_account: &'a str,
}

/// Balances needed to derive validator economics metrics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountBalances {
    /// Identity-account balance in lamports.
    pub identity_lamports: u64,
    /// Vote-account balance in lamports.
    pub vote_account_lamports: u64,
    /// Current rent-exempt minimum for the vote account, when its data size is known.
    pub vote_account_rent_exempt_minimum_lamports: Option<u64>,
}

/// Current balances and derived vote-account funding runway for one watched validator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValidatorFundingMetrics {
    /// Identity-account balance in lamports.
    pub identity_balance_lamports: u64,
    /// Vote-account balance in lamports.
    pub vote_account_balance_lamports: u64,
    /// Rent-exempt minimum for the vote account.
    pub vote_account_rent_exempt_minimum_lamports: Option<u64>,
    /// Vote-account balance above rent exemption available for future VAT charges.
    pub vat_reserve_lamports: Option<u64>,
    /// Whole future 0.8 SOL VAT charges covered by the available reserve.
    pub vat_epochs_available: Option<u64>,
    /// Whether the reserve covers the next 0.8 SOL VAT charge.
    pub vat_next_epoch_funded: Option<bool>,
}

/// Configured-target provider and recent block-fee economics.
#[derive(Clone, Debug, PartialEq)]
pub struct EconomicsMetrics {
    /// Validator identity account address.
    pub identity: String,
    /// Validator vote-account address.
    pub vote_account: String,
    /// Identity-account balance in lamports.
    pub identity_balance_lamports: u64,
    /// Vote-account balance in lamports.
    pub vote_account_balance_lamports: u64,
    /// Rent-exempt minimum for the vote account.
    pub vote_account_rent_exempt_minimum_lamports: Option<u64>,
    /// Vote-account balance above rent exemption available for future VAT charges.
    pub vat_reserve_lamports: Option<u64>,
    /// Whole future 0.8 SOL VAT charges covered by the available reserve.
    pub vat_epochs_available: Option<u64>,
    /// Whether the reserve covers the next 0.8 SOL VAT charge.
    pub vat_next_epoch_funded: Option<bool>,
    /// Settled Jito MEV reward, when a configured Jito route has published the epoch.
    pub jito_mev_reward: Option<JitoMevReward>,
    /// Completed-epoch MEV capture when a configured Trillium route has data.
    pub trillium_mev_capture: Option<TrilliumMevCapture>,
    /// Recent confirmed validator fee rewards, including Harmonic priority-fee tips.
    pub recent_block_fee_rewards: Option<RecentBlockFeeRewards>,
}

/// Result of matching a watched validator to one `getClusterNodes` response.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum GossipObservation {
    /// No configured gossip route succeeded, so presence is unknown.
    #[default]
    Unavailable,
    /// The command succeeded but did not contain the watched identity.
    Missing,
    /// The command returned the watched identity and its advertised metadata.
    Present {
        /// Validator client version when advertised.
        version: Option<String>,
        /// Feature-set identifier when advertised.
        feature_set: Option<u32>,
        /// Shred protocol version when advertised.
        shred_version: Option<u16>,
    },
}

/// Per-validator values exported for explicitly watched validators.
#[derive(Clone, Debug, PartialEq)]
pub struct WatchedValidatorMetrics {
    /// Validator identity account address.
    pub identity: String,
    /// Validator vote-account address.
    pub vote_account: String,
    /// Current or delinquent cluster status.
    pub status: ValidatorStatus,
    /// Presence and software metadata in the latest available public gossip view.
    pub gossip: GossipObservation,
    /// Metric scopes to which this validator contributes.
    pub scopes: Vec<String>,
    /// Activated stake delegated to the vote account, in lamports.
    pub activated_stake_lamports: u64,
    /// Current account balances and VAT runway when economics collection is enabled.
    pub funding: Option<ValidatorFundingMetrics>,
    /// Vote credits earned in the bounded sample epoch.
    pub latest_epoch_vote_credits: Option<u64>,
    /// Earned vote credits as a percentage of the scope's ideal credits.
    pub vote_efficiency_pct: Option<f64>,
    /// Current slot minus the validator's last voted slot.
    pub last_vote_slot_lag: u64,
    /// Current slot minus the validator's rooted slot.
    pub root_slot_lag: u64,
    /// Vote-account commission percentage.
    pub commission_pct: u8,
    /// Inflation reward earned in the bounded reward epoch, when available.
    pub latest_epoch_reward_lamports: Option<u64>,
    /// Blocks produced during the bounded reward epoch.
    pub latest_epoch_rewarded_blocks_produced: u64,
    /// Blocks produced in assigned leader slots during the current epoch.
    pub blocks_produced: u64,
    /// Assigned current-epoch leader slots without a produced block.
    pub blocks_missed: u64,
    /// Percentage of assigned current-epoch leader slots with a produced block.
    pub block_production_rate: f64,
    /// Total assignments in the current epoch when its schedule was collected.
    pub current_epoch_scheduled_leader_slots: Option<u64>,
    /// Assigned leader slots later than the current slot.
    pub future_leader_slots: u64,
    /// Next assigned leader slot, when one remains.
    pub next_leader_slot: Option<u64>,
    /// Slots from the current slot to the next assignment.
    pub slots_until_next_leader: Option<u64>,
    /// Estimated seconds from now to the next assignment.
    pub seconds_until_next_leader: Option<f64>,
    /// Earliest current- or next-epoch assignments, ordered by absolute slot.
    pub upcoming_leader_slots: Vec<u64>,
}

/// Normalized validator record used as the aggregation input.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidatorRecord {
    /// Validator identity account address.
    pub identity: String,
    /// Validator vote-account address.
    pub vote_account: String,
    /// Current or delinquent cluster status.
    pub status: ValidatorStatus,
    /// Activated stake delegated to the vote account, in lamports.
    pub activated_stake_lamports: u64,
    /// Vote-account commission percentage.
    pub commission: u8,
    /// Most recent slot voted by the validator.
    pub last_vote: u64,
    /// Most recent rooted slot reported by the validator.
    pub root_slot: u64,
    /// Latest bounded vote-credit sample.
    pub latest_epoch_credits: Option<EpochCreditsSample>,
    /// Latest bounded inflation-reward sample.
    pub latest_epoch_reward: Option<RewardSample>,
    /// Blocks produced during the reward sample epoch.
    pub latest_epoch_rewarded_blocks_produced: u64,
    /// Leader slots assigned during the current epoch.
    pub current_epoch_leader_slots: u64,
    /// Blocks produced during the current epoch.
    pub current_epoch_blocks_produced: u64,
    /// Total assignments in the current epoch when its schedule was collected.
    pub current_epoch_scheduled_leader_slots: Option<u64>,
    /// Assigned leader slots later than the current slot.
    pub future_leader_slots: u64,
    /// Next assigned leader slot, when one remains.
    pub next_leader_slot: Option<u64>,
    /// Slots from the current slot to the next assignment.
    pub slots_until_next_leader: Option<u64>,
    /// Estimated seconds from now to the next assignment.
    pub seconds_until_next_leader: Option<f64>,
    /// Earliest current- or next-epoch assignments for a watched validator.
    pub upcoming_leader_slots: Vec<u64>,
    /// Metric scopes to which this validator contributes.
    pub scopes: Vec<String>,
}

impl ValidatorRecord {
    /// Reports whether the record belongs to the built-in watched scope.
    pub fn is_watched(&self) -> bool {
        self.scopes.iter().any(|scope| scope == LABEL_SCOPE_WATCHED)
    }

    /// Returns saturating last-vote lag at `current_slot`.
    pub fn last_vote_slot_lag(&self, current_slot: u64) -> u64 {
        current_slot.saturating_sub(self.last_vote)
    }

    /// Returns saturating root lag at `current_slot`.
    pub fn root_slot_lag(&self, current_slot: u64) -> u64 {
        current_slot.saturating_sub(self.root_slot)
    }

    /// Returns credits only when the record and aggregate sample epochs match.
    pub fn matching_epoch_credits(&self, sample_epoch: Option<u64>) -> Option<u64> {
        self.latest_epoch_credits
            .as_ref()
            .filter(|credits| Some(credits.epoch) == sample_epoch)
            .map(|credits| credits.earned)
    }

    /// Returns reward lamports only when the record and aggregate epochs match.
    pub fn matching_epoch_reward_lamports(&self, reward_sample_epoch: Option<u64>) -> Option<u64> {
        self.latest_epoch_reward
            .as_ref()
            .filter(|reward| Some(reward.epoch) == reward_sample_epoch)
            .map(|reward| reward.amount_lamports)
    }

    /// Returns vote efficiency only for matching samples with a nonzero ideal.
    pub fn vote_efficiency_pct(
        &self,
        ideal_epoch_credits: Option<u64>,
        sample_epoch: Option<u64>,
    ) -> Option<f64> {
        let credits = self.matching_epoch_credits(sample_epoch)?;
        let ideal = ideal_epoch_credits.filter(|ideal| *ideal > 0)?;

        Some(ratio(credits, ideal))
    }

    /// Returns assigned current-epoch slots without a produced block.
    pub fn blocks_missed(&self) -> u64 {
        self.current_epoch_leader_slots
            .saturating_sub(self.current_epoch_blocks_produced)
    }
}

/// Aggregated health, voting, rewards, and leadership values for one scope.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScopeMetrics {
    /// Number of current validators.
    pub current_count: u64,
    /// Number of delinquent validators.
    pub delinquent_count: u64,
    /// Activated stake delegated to current validators, in lamports.
    pub current_stake_lamports: u64,
    /// Activated stake delegated to delinquent validators, in lamports.
    pub delinquent_stake_lamports: u64,
    /// Sum of vote credits earned in the bounded sample epoch.
    pub latest_epoch_vote_credits: Option<u64>,
    /// Complete sum of inflation rewards in the bounded reward epoch, when available.
    pub latest_epoch_reward_lamports: Option<u64>,
    /// Sum of blocks produced during the bounded reward epoch.
    pub latest_epoch_rewarded_blocks_produced: u64,
    /// Stake-weighted vote efficiency percentage.
    pub vote_efficiency_pct: Option<f64>,
    /// Stake-weighted last-vote slot lag.
    pub last_vote_slot_lag: f64,
    /// Stake-weighted root slot lag.
    pub root_slot_lag: f64,
    /// Stake-weighted commission percentage.
    pub average_commission_pct: f64,
    /// Blocks produced in assigned leader slots during the current epoch.
    pub blocks_produced: u64,
    /// Assigned current-epoch leader slots without a produced block.
    pub blocks_missed: u64,
    /// Percentage of assigned current-epoch leader slots with a produced block.
    pub block_production_rate: f64,
    /// Assigned leader slots later than the current slot.
    pub future_leader_slots: u64,
}

impl ScopeMetrics {
    /// Returns current plus delinquent activated stake.
    pub fn activated_stake_lamports(&self) -> u64 {
        self.current_stake_lamports + self.delinquent_stake_lamports
    }
}

/// Cluster rates derived from the latest Solana performance sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClusterPerformanceMetrics {
    /// Average slot duration when the sample contains at least one slot.
    pub average_slot_time_seconds: Option<f64>,
    /// All processed transactions per second, including vote transactions.
    pub transactions_per_second: f64,
    /// Vote transactions per second when the endpoint supplies the transaction split.
    pub vote_transactions_per_second: Option<f64>,
    /// Non-vote transactions per second when supplied by the endpoint.
    pub non_vote_transactions_per_second: Option<f64>,
    /// Slots completed per second during the sample.
    pub slots_per_second: f64,
}

/// One complete collection result consumed by metrics rendering.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    /// Stable network label.
    pub network: String,
    /// Current epoch.
    pub epoch: u64,
    /// Current absolute slot.
    pub slot: u64,
    /// Total number of slots in the current epoch.
    pub epoch_slots: u64,
    /// Percentage of the current epoch completed.
    pub epoch_progress_pct: f64,
    /// Slots remaining in the current epoch.
    pub epoch_slots_remaining: u64,
    /// Cluster rates derived from the latest recent-performance sample.
    pub cluster_performance: Option<ClusterPerformanceMetrics>,
    /// Estimated Unix timestamp at which the current epoch will end.
    pub estimated_epoch_end_unixtime: Option<f64>,
    /// Epoch represented by vote-credit and efficiency values.
    pub vote_credits_sample_epoch: Option<u64>,
    /// Epoch represented by reward values.
    pub reward_sample_epoch: Option<u64>,
    /// Aggregated values keyed by stable scope label.
    pub scopes: BTreeMap<String, ScopeMetrics>,
    /// Detailed values for explicitly watched validators.
    pub watched_validators: Vec<WatchedValidatorMetrics>,
    /// Successful source provenance for collected commands.
    pub data_sources: Vec<SnapshotDataSource>,
    /// Enabled collection-mode outcomes for this polling round.
    pub mode_statuses: Vec<ModeStatus>,
    /// Node-local health values when a node-status route succeeds.
    pub node: Option<NodeMetrics>,
    /// Operator economics values when economics mode is enabled and collected.
    pub economics: Option<EconomicsMetrics>,
}

/// Success state for one enabled collection mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModeStatus {
    /// Collection mode.
    pub mode: ModeId,
    /// Whether every required input for the mode was collected.
    pub up: bool,
}

/// Snapshot-stable provenance for one successfully collected command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotDataSource {
    /// Mode that owns the command.
    pub mode: ModeId,
    /// Command that supplied data.
    pub command: CommandId,
    /// Stable configured connector name.
    pub connector: Arc<str>,
    /// Operator-declared provider name.
    pub provider: Arc<str>,
    /// Transport used by the command.
    pub transport: Transport,
    /// Solana RPC profile declared by the connector.
    pub solana_rpc_compatibility: Option<SolanaRpcCompatibility>,
    /// Provider feature required by the command, if any.
    pub feature: Option<ProviderFeature>,
    /// One-based fallback position of the successful connector.
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

fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        return 0.0;
    }
    numerator as f64 / denominator as f64 * 100.0
}
