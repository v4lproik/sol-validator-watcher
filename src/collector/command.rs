//! Typed watcher commands mapped to concrete Solana RPC operations.
//!
//! Commands describe one operation and output type. Route selection and fallback
//! remain the responsibility of the source chain.

use super::{
    AccountBalances, BlockProduction, ClusterPerformanceMetrics, EpochInfo, NodeHealth,
    NodeRpcStatus, RewardSample, RpcClient, VoteAccounts,
};
use crate::mode::CommandId;
use anyhow::{Context, Result};
use std::collections::HashMap;

/// One typed collector operation executable by an RPC client.
pub(crate) trait SourceCommand {
    /// Stable routing identifier for this operation.
    const ID: CommandId;
    /// Decoded value returned on success.
    type Output;

    /// Executes the operation against one already-selected connector.
    async fn execute(&self, client: &RpcClient) -> Result<Self::Output>;
}

/// Required Solana RPC health command.
pub(crate) struct GetHealth;

impl SourceCommand for GetHealth {
    const ID: CommandId = CommandId::RpcHealth;
    type Output = NodeHealth;

    async fn execute(&self, client: &RpcClient) -> Result<Self::Output> {
        client.node_health().await
    }
}

/// Optional node-local identity, software, block-height, and health command.
pub(crate) struct GetNodeStatus;

impl SourceCommand for GetNodeStatus {
    const ID: CommandId = CommandId::NodeStatus;
    type Output = NodeRpcStatus;

    async fn execute(&self, client: &RpcClient) -> Result<Self::Output> {
        client
            .node_status()
            .await
            .context("node-local status collection failed")
    }
}

/// Required current epoch-information command.
pub(crate) struct GetEpochInfo;

impl SourceCommand for GetEpochInfo {
    const ID: CommandId = CommandId::EpochInfo;
    type Output = EpochInfo;

    async fn execute(&self, client: &RpcClient) -> Result<Self::Output> {
        client.get_epoch_info().await.context("getEpochInfo failed")
    }
}

/// Required vote-account command.
pub(crate) struct GetVoteAccounts {
    /// Optional cluster-specific distance used to classify delinquent accounts.
    pub delinquent_slot_distance: Option<u64>,
}

impl SourceCommand for GetVoteAccounts {
    const ID: CommandId = CommandId::VoteAccounts;
    type Output = VoteAccounts;

    async fn execute(&self, client: &RpcClient) -> Result<Self::Output> {
        client
            .get_vote_accounts(self.delinquent_slot_distance)
            .await
            .context("getVoteAccounts failed")
    }
}

/// Optional recent-performance command that derives cluster rates and slot duration.
pub(crate) struct GetRecentPerformanceSamples;

impl SourceCommand for GetRecentPerformanceSamples {
    const ID: CommandId = CommandId::RecentPerformanceSamples;
    type Output = Option<ClusterPerformanceMetrics>;

    async fn execute(&self, client: &RpcClient) -> Result<Self::Output> {
        client
            .get_cluster_performance_metrics()
            .await
            .context("getRecentPerformanceSamples failed")
    }
}

/// Optional block-production command for the current epoch range.
pub(crate) struct GetCurrentBlockProduction;

impl SourceCommand for GetCurrentBlockProduction {
    const ID: CommandId = CommandId::CurrentBlockProduction;
    type Output = BlockProduction;

    async fn execute(&self, client: &RpcClient) -> Result<Self::Output> {
        client
            .get_current_epoch_block_production()
            .await
            .context("getBlockProduction for current epoch failed")
    }
}

/// Optional leader-schedule command covering the current slot.
pub(crate) struct GetLeaderSchedule {
    /// Absolute slot used to select the schedule.
    pub slot: u64,
}

impl SourceCommand for GetLeaderSchedule {
    const ID: CommandId = CommandId::CurrentLeaderSchedule;
    type Output = Option<HashMap<String, Vec<u64>>>;

    async fn execute(&self, client: &RpcClient) -> Result<Self::Output> {
        client
            .get_leader_schedule(self.slot)
            .await
            .context("getLeaderSchedule failed")
    }
}

/// Optional leader-schedule command for the next epoch boundary.
pub(crate) struct GetNextLeaderSchedule {
    /// First absolute slot of the next epoch.
    pub slot: u64,
}

impl SourceCommand for GetNextLeaderSchedule {
    const ID: CommandId = CommandId::NextLeaderSchedule;
    type Output = Option<HashMap<String, Vec<u64>>>;

    async fn execute(&self, client: &RpcClient) -> Result<Self::Output> {
        client
            .get_leader_schedule(self.slot)
            .await
            .context("getLeaderSchedule for next epoch failed")
    }
}

/// Required identity and vote-account balance command.
pub(crate) struct GetAccountBalances<'a> {
    /// Validator identity account address.
    pub identity: &'a str,
    /// Validator vote-account address.
    pub vote_account: &'a str,
}

impl SourceCommand for GetAccountBalances<'_> {
    const ID: CommandId = CommandId::AccountBalances;
    type Output = AccountBalances;

    async fn execute(&self, client: &RpcClient) -> Result<Self::Output> {
        client
            .get_account_balances(self.identity, self.vote_account)
            .await
            .context("account balance collection failed")
    }
}

/// Required prior-epoch inflation-reward command.
pub(crate) struct GetInflationRewards<'a> {
    /// Vote accounts whose rewards are requested.
    pub addresses: &'a [String],
    /// Reward epoch to query.
    pub epoch: u64,
}

impl SourceCommand for GetInflationRewards<'_> {
    const ID: CommandId = CommandId::InflationRewards;
    type Output = HashMap<String, RewardSample>;

    async fn execute(&self, client: &RpcClient) -> Result<Self::Output> {
        client
            .get_inflation_rewards(self.addresses, self.epoch)
            .await
            .context("getInflationReward failed")
    }
}

/// Required prior-epoch block-production command.
pub(crate) struct GetRewardEpochBlockProduction {
    /// First slot of the current epoch.
    pub epoch_start_slot: u64,
    /// Number of slots in the current epoch.
    pub slots_in_epoch: u64,
}

impl SourceCommand for GetRewardEpochBlockProduction {
    const ID: CommandId = CommandId::RewardEpochBlockProduction;
    type Output = BlockProduction;

    async fn execute(&self, client: &RpcClient) -> Result<Self::Output> {
        let minimum_ledger_slot = client
            .get_minimum_ledger_slot()
            .await
            .context("minimumLedgerSlot failed before reward epoch block production")?;
        let range = super::reward_epoch_block_range(
            self.epoch_start_slot,
            self.slots_in_epoch,
            minimum_ledger_slot,
        )
        .context("reward epoch block production history is unavailable")?;

        client
            .get_block_production_for_range(range.first_slot, range.last_slot)
            .await
            .context("getBlockProduction for reward epoch failed")
    }
}
