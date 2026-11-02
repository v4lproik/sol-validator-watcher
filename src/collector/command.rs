// Implementations retain the explicit `Send` future promised by `SourceCommand`.
#![allow(clippy::manual_async_fn)]

use super::{reward_epoch_block_range, BlockProduction, EpochInfo, RewardSample, VoteAccounts};
use crate::{
    provider::CommandId,
    source::{Connector, SourceCommand},
};
use anyhow::{Context, Result};
use std::{collections::HashMap, future::Future};

pub(super) struct GetEpochInfo;

impl SourceCommand for GetEpochInfo {
    type Output = EpochInfo;

    const ID: CommandId = CommandId::EpochInfo;

    fn execute<'a>(
        &'a self,
        connector: &'a Connector,
    ) -> impl Future<Output = Result<Self::Output>> + Send + 'a {
        async move {
            connector
                .rpc()?
                .get_epoch_info()
                .await
                .context("getEpochInfo failed")
        }
    }
}

pub(super) struct GetVoteAccounts {
    pub(super) delinquent_slot_distance: Option<u64>,
}

impl SourceCommand for GetVoteAccounts {
    type Output = VoteAccounts;

    const ID: CommandId = CommandId::VoteAccounts;

    fn execute<'a>(
        &'a self,
        connector: &'a Connector,
    ) -> impl Future<Output = Result<Self::Output>> + Send + 'a {
        async move {
            connector
                .rpc()?
                .get_vote_accounts(self.delinquent_slot_distance)
                .await
                .context("getVoteAccounts failed")
        }
    }
}

pub(super) struct GetCurrentBlockProduction;

impl SourceCommand for GetCurrentBlockProduction {
    type Output = BlockProduction;

    const ID: CommandId = CommandId::CurrentBlockProduction;

    fn execute<'a>(
        &'a self,
        connector: &'a Connector,
    ) -> impl Future<Output = Result<Self::Output>> + Send + 'a {
        async move {
            connector
                .rpc()?
                .get_current_epoch_block_production()
                .await
                .context("getBlockProduction failed")
        }
    }
}

pub(super) struct GetCurrentLeaderSchedule {
    pub(super) slot: u64,
}

impl SourceCommand for GetCurrentLeaderSchedule {
    type Output = Option<HashMap<String, Vec<u64>>>;

    const ID: CommandId = CommandId::CurrentLeaderSchedule;

    fn execute<'a>(
        &'a self,
        connector: &'a Connector,
    ) -> impl Future<Output = Result<Self::Output>> + Send + 'a {
        async move {
            connector
                .rpc()?
                .get_leader_schedule(self.slot)
                .await
                .context("getLeaderSchedule for current epoch failed")
        }
    }
}

pub(super) struct GetNextLeaderSchedule {
    pub(super) slot: u64,
}

impl SourceCommand for GetNextLeaderSchedule {
    type Output = Option<HashMap<String, Vec<u64>>>;

    const ID: CommandId = CommandId::NextLeaderSchedule;

    fn execute<'a>(
        &'a self,
        connector: &'a Connector,
    ) -> impl Future<Output = Result<Self::Output>> + Send + 'a {
        async move {
            connector
                .rpc()?
                .get_leader_schedule(self.slot)
                .await
                .context("getLeaderSchedule for next epoch failed")
        }
    }
}

pub(super) struct GetInflationRewards<'a> {
    pub(super) addresses: &'a [String],
    pub(super) epoch: u64,
}

impl SourceCommand for GetInflationRewards<'_> {
    type Output = HashMap<String, RewardSample>;

    const ID: CommandId = CommandId::InflationRewards;

    fn execute<'a>(
        &'a self,
        connector: &'a Connector,
    ) -> impl Future<Output = Result<Self::Output>> + Send + 'a {
        async move {
            connector
                .rpc()?
                .get_inflation_rewards(self.addresses, self.epoch)
                .await
                .context("getInflationReward failed")
        }
    }
}

pub(super) struct GetRewardEpochBlockProduction {
    pub(super) epoch_start_slot: u64,
    pub(super) slots_in_epoch: u64,
}

impl SourceCommand for GetRewardEpochBlockProduction {
    type Output = BlockProduction;

    const ID: CommandId = CommandId::RewardEpochBlockProduction;

    fn execute<'a>(
        &'a self,
        connector: &'a Connector,
    ) -> impl Future<Output = Result<Self::Output>> + Send + 'a {
        async move {
            let client = connector.rpc()?;
            let minimum_ledger_slot = client.get_minimum_ledger_slot().await.ok();
            let range = reward_epoch_block_range(
                self.epoch_start_slot,
                self.slots_in_epoch,
                minimum_ledger_slot,
            )
            .context("connector does not retain enough reward-epoch ledger history")?;

            client
                .get_block_production_for_range(range.first_slot, range.last_slot)
                .await
                .context("getBlockProduction for reward epoch failed")
        }
    }
}
