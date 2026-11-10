//! Typed watcher commands mapped to concrete Solana RPC operations.
//!
//! Commands describe one operation and output type. Route selection and fallback
//! remain the responsibility of the source chain.

use super::{
    AccountBalanceTarget, AccountBalances, BlockProduction, ClusterNode, ClusterPerformanceMetrics,
    EpochInfo, JitoClient, JitoMevReward, NodeHealth, NodeRpcStatus, RecentBlockFeeRewards,
    RewardSample, RpcClient, TrilliumClient, TrilliumMevCapture, VoteAccounts,
};
use crate::mode::CommandId;
use anyhow::{Context, Result};
use std::collections::HashMap;

/// Typed command clients constructed once for one configured connector.
pub(crate) struct CommandClients {
    rpc: Option<RpcClient>,
    jito_api: Option<JitoClient>,
    trillium_api: Option<TrilliumClient>,
}

impl CommandClients {
    /// Groups the connector's available typed command clients.
    pub(crate) fn new(
        rpc: Option<RpcClient>,
        jito_api: Option<JitoClient>,
        trillium_api: Option<TrilliumClient>,
    ) -> Self {
        Self {
            rpc,
            jito_api,
            trillium_api,
        }
    }

    /// Returns the connector's standard Solana RPC client.
    pub(crate) fn rpc(&self) -> Option<&RpcClient> {
        self.rpc.as_ref()
    }

    /// Returns the connector's Jito validator-API client.
    pub(crate) fn jito_api(&self) -> Option<&JitoClient> {
        self.jito_api.as_ref()
    }

    /// Returns the connector's Trillium validator-analytics client.
    pub(crate) fn trillium_api(&self) -> Option<&TrilliumClient> {
        self.trillium_api.as_ref()
    }
}

/// Compile-time selection of the client type used by a typed command.
pub(crate) trait CommandTransport {
    /// Concrete client accepted by commands on this transport.
    type Client;

    /// Selects the transport client from one configured connector.
    fn client(clients: &CommandClients) -> Option<&Self::Client>;
}

/// Standard Solana JSON-RPC command transport.
pub(crate) enum RpcCommandTransport {}

impl CommandTransport for RpcCommandTransport {
    type Client = RpcClient;

    fn client(clients: &CommandClients) -> Option<&Self::Client> {
        clients.rpc.as_ref()
    }
}

/// Jito validator HTTP API command transport.
pub(crate) enum JitoApiCommandTransport {}

impl CommandTransport for JitoApiCommandTransport {
    type Client = JitoClient;

    fn client(clients: &CommandClients) -> Option<&Self::Client> {
        clients.jito_api.as_ref()
    }
}

/// Trillium validator analytics HTTP API command transport.
pub(crate) enum TrilliumApiCommandTransport {}

impl CommandTransport for TrilliumApiCommandTransport {
    type Client = TrilliumClient;

    fn client(clients: &CommandClients) -> Option<&Self::Client> {
        clients.trillium_api.as_ref()
    }
}

/// One typed collector operation executable by its declared transport client.
pub(crate) trait SourceCommand {
    /// Stable routing identifier for this operation.
    const ID: CommandId;
    /// Client transport selected at compile time for this command.
    type Transport: CommandTransport;
    /// Decoded value returned on success.
    type Output;

    /// Executes the operation against one already-selected connector.
    async fn execute(
        &self,
        client: &<Self::Transport as CommandTransport>::Client,
    ) -> Result<Self::Output>;
}

/// Required Solana RPC health command.
pub(crate) struct GetHealth;

impl SourceCommand for GetHealth {
    const ID: CommandId = CommandId::RpcHealth;
    type Transport = RpcCommandTransport;
    type Output = NodeHealth;

    async fn execute(&self, client: &RpcClient) -> Result<Self::Output> {
        client.node_health().await
    }
}

/// Optional node-local identity, software, block-height, and health command.
pub(crate) struct GetNodeStatus;

impl SourceCommand for GetNodeStatus {
    const ID: CommandId = CommandId::NodeStatus;
    type Transport = RpcCommandTransport;
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
    type Transport = RpcCommandTransport;
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
    type Transport = RpcCommandTransport;
    type Output = VoteAccounts;

    async fn execute(&self, client: &RpcClient) -> Result<Self::Output> {
        client
            .get_vote_accounts(self.delinquent_slot_distance)
            .await
            .context("getVoteAccounts failed")
    }
}

/// Optional public gossip-peer and validator-version command.
pub(crate) struct GetClusterNodes;

impl SourceCommand for GetClusterNodes {
    const ID: CommandId = CommandId::ClusterNodes;
    type Transport = RpcCommandTransport;
    type Output = Vec<ClusterNode>;

    async fn execute(&self, client: &RpcClient) -> Result<Self::Output> {
        client
            .get_cluster_nodes()
            .await
            .context("getClusterNodes failed")
    }
}

/// Optional recent-performance command that derives cluster rates and slot duration.
pub(crate) struct GetRecentPerformanceSamples;

impl SourceCommand for GetRecentPerformanceSamples {
    const ID: CommandId = CommandId::RecentPerformanceSamples;
    type Transport = RpcCommandTransport;
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
    type Transport = RpcCommandTransport;
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
    type Transport = RpcCommandTransport;
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
    type Transport = RpcCommandTransport;
    type Output = Option<HashMap<String, Vec<u64>>>;

    async fn execute(&self, client: &RpcClient) -> Result<Self::Output> {
        client
            .get_leader_schedule(self.slot)
            .await
            .context("getLeaderSchedule for next epoch failed")
    }
}

/// Required watched-validator account balance command.
pub(crate) struct GetAccountBalances<'a> {
    /// Validator identity and vote-account pairs requested in stable output order.
    pub targets: &'a [AccountBalanceTarget<'a>],
}

impl SourceCommand for GetAccountBalances<'_> {
    const ID: CommandId = CommandId::AccountBalances;
    type Transport = RpcCommandTransport;
    type Output = Vec<Option<AccountBalances>>;

    async fn execute(&self, client: &RpcClient) -> Result<Self::Output> {
        client
            .get_account_balances(self.targets)
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
    type Transport = RpcCommandTransport;
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
    type Transport = RpcCommandTransport;
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

/// Optional settled prior-epoch Jito MEV reward command.
pub(crate) struct GetJitoMevReward<'a> {
    /// Validator vote account whose rewards are requested.
    pub vote_account: &'a str,
    /// Settled reward epoch to query.
    pub epoch: u64,
}

impl SourceCommand for GetJitoMevReward<'_> {
    const ID: CommandId = CommandId::JitoMevRewards;
    type Transport = JitoApiCommandTransport;
    type Output = Option<JitoMevReward>;

    async fn execute(&self, client: &JitoClient) -> Result<Self::Output> {
        client
            .get_mev_reward(self.vote_account, self.epoch)
            .await
            .context("Jito MEV reward collection failed")
    }
}

/// Optional latest completed-epoch Trillium MEV capture command.
pub(crate) struct GetTrilliumMevCapture<'a> {
    /// Validator vote account whose completed-epoch capture is requested.
    pub vote_account: &'a str,
}

impl SourceCommand for GetTrilliumMevCapture<'_> {
    const ID: CommandId = CommandId::TrilliumMevCapture;
    type Transport = TrilliumApiCommandTransport;
    type Output = Option<TrilliumMevCapture>;

    async fn execute(&self, client: &TrilliumClient) -> Result<Self::Output> {
        client
            .get_latest_mev_capture(self.vote_account)
            .await
            .context("Trillium MEV capture collection failed")
    }
}

/// Optional recent confirmed validator block-fee reward command.
pub(crate) struct GetRecentBlockFeeRewards<'a> {
    /// Recent assigned absolute slots, bounded by the RPC client contract.
    pub slots: &'a [u64],
    /// Validator identity expected to receive each produced block's fee reward.
    pub identity: &'a str,
}

impl SourceCommand for GetRecentBlockFeeRewards<'_> {
    const ID: CommandId = CommandId::RecentBlockFeeRewards;
    type Transport = RpcCommandTransport;
    type Output = Option<RecentBlockFeeRewards>;

    async fn execute(&self, client: &RpcClient) -> Result<Self::Output> {
        client
            .get_recent_block_fee_rewards(self.slots, self.identity)
            .await
            .context("recent validator block-fee reward collection failed")
    }
}
