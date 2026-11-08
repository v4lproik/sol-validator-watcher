//! Solana RPC collection and snapshot construction.
//!
//! The submodules separate typed commands, wire transport, domain records, pure
//! aggregation, and collection orchestration behind this facade.

mod aggregate;
pub mod command;
mod jito;
mod model;
mod rpc;
mod snapshot;
mod trillium;

pub use model::{
    ClusterPerformanceMetrics, GossipObservation, ModeStatus, NodeHealth, RecentBlockFeeRewards,
    Snapshot, SnapshotDataSource, WatchedValidatorMetrics,
};
pub use rpc::RpcClient;
pub use snapshot::{collect_snapshot, ensure_economics_supported};

use aggregate::reward_epoch_block_range;
pub(crate) use jito::JitoClient;
pub(crate) use model::MAX_UPCOMING_LEADER_SLOTS;
pub(crate) use model::{
    AccountBalanceTarget, AccountBalances, JitoMevReward, NodeRpcStatus, RewardSample,
    TrilliumBlockRewards, TrilliumMevCapture, TrilliumTransactionMetrics,
};
#[cfg(test)]
pub(crate) use model::{
    EconomicsMetrics, EpochCreditsSample, NodeMetrics, NodeVotingStatus, ScopeMetrics,
    ValidatorFundingMetrics, ValidatorRecord, ValidatorStatus,
};
pub(crate) use rpc::{BlockProduction, ClusterNode, EpochInfo, VoteAccounts};
pub(crate) use trillium::TrilliumClient;

#[cfg(test)]
use aggregate::{
    aggregate_records, build_record, build_validator_funding_metrics,
    build_watched_validator_metrics, estimated_epoch_end_unixtime_at, ratio,
    resolve_economics_target, EconomicsTarget, RecordBuildContext,
};
#[cfg(test)]
use rpc::{
    GetBlockProductionConfig, RpcValueResponse, SlotRange, VoteAccountInfo,
    MAX_BLOCK_PRODUCTION_RANGE_SLOTS,
};

#[cfg(test)]
mod tests;
