use super::{BlockProduction, EpochInfo, RewardSample, RpcClient, VoteAccounts};
use crate::mode::CommandId;
use anyhow::{Context, Result};
use std::collections::HashMap;

pub(crate) trait SourceCommand {
    const ID: CommandId;
    type Output;

    async fn execute(&self, client: &RpcClient) -> Result<Self::Output>;
}

pub(crate) struct GetHealth;

impl SourceCommand for GetHealth {
    const ID: CommandId = CommandId::RpcHealth;
    type Output = ();

    async fn execute(&self, client: &RpcClient) -> Result<Self::Output> {
        client.ensure_healthy().await
    }
}

pub(crate) struct GetEpochInfo;

impl SourceCommand for GetEpochInfo {
    const ID: CommandId = CommandId::EpochInfo;
    type Output = EpochInfo;

    async fn execute(&self, client: &RpcClient) -> Result<Self::Output> {
        client.get_epoch_info().await.context("getEpochInfo failed")
    }
}

pub(crate) struct GetVoteAccounts {
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

pub(crate) struct GetLeaderSchedule {
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

pub(crate) struct GetNextLeaderSchedule {
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

pub(crate) struct GetInflationRewards<'a> {
    pub addresses: &'a [String],
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

pub(crate) struct GetRewardEpochBlockProduction {
    pub epoch_start_slot: u64,
    pub slots_in_epoch: u64,
}

impl SourceCommand for GetRewardEpochBlockProduction {
    const ID: CommandId = CommandId::RewardEpochBlockProduction;
    type Output = BlockProduction;

    async fn execute(&self, client: &RpcClient) -> Result<Self::Output> {
        let minimum_ledger_slot = client.get_minimum_ledger_slot().await.ok();
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
