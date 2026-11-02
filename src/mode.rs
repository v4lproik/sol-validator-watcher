use serde::{Deserialize, Serialize};

pub const MODE_COUNT: usize = 4;
pub const COMMAND_COUNT: usize = 9;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Transport {
    Rpc,
    Websocket,
}

impl Transport {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rpc => "rpc",
            Self::Websocket => "websocket",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModeId {
    Health,
    Consensus,
    Leadership,
    Economics,
}

impl ModeId {
    pub const ALL: [Self; MODE_COUNT] = [
        Self::Health,
        Self::Consensus,
        Self::Leadership,
        Self::Economics,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Health => "health",
            Self::Consensus => "consensus",
            Self::Leadership => "leadership",
            Self::Economics => "economics",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandId {
    RpcHealth,
    WsHealth,
    EpochInfo,
    VoteAccounts,
    CurrentBlockProduction,
    CurrentLeaderSchedule,
    NextLeaderSchedule,
    InflationRewards,
    RewardEpochBlockProduction,
}

impl CommandId {
    pub const ALL: [Self; COMMAND_COUNT] = [
        Self::RpcHealth,
        Self::WsHealth,
        Self::EpochInfo,
        Self::VoteAccounts,
        Self::CurrentBlockProduction,
        Self::CurrentLeaderSchedule,
        Self::NextLeaderSchedule,
        Self::InflationRewards,
        Self::RewardEpochBlockProduction,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RpcHealth => "rpc-health",
            Self::WsHealth => "ws-health",
            Self::EpochInfo => "epoch-info",
            Self::VoteAccounts => "vote-accounts",
            Self::CurrentBlockProduction => "current-block-production",
            Self::CurrentLeaderSchedule => "current-leader-schedule",
            Self::NextLeaderSchedule => "next-leader-schedule",
            Self::InflationRewards => "inflation-rewards",
            Self::RewardEpochBlockProduction => "reward-epoch-block-production",
        }
    }

    pub const fn mode(self) -> ModeId {
        match self {
            Self::RpcHealth | Self::WsHealth => ModeId::Health,
            Self::EpochInfo | Self::VoteAccounts => ModeId::Consensus,
            Self::CurrentBlockProduction
            | Self::CurrentLeaderSchedule
            | Self::NextLeaderSchedule => ModeId::Leadership,
            Self::InflationRewards | Self::RewardEpochBlockProduction => ModeId::Economics,
        }
    }

    pub const fn transport(self) -> Transport {
        match self {
            Self::WsHealth => Transport::Websocket,
            Self::RpcHealth
            | Self::EpochInfo
            | Self::VoteAccounts
            | Self::CurrentBlockProduction
            | Self::CurrentLeaderSchedule
            | Self::NextLeaderSchedule
            | Self::InflationRewards
            | Self::RewardEpochBlockProduction => Transport::Rpc,
        }
    }

    pub const fn index(self) -> usize {
        match self {
            Self::RpcHealth => 0,
            Self::WsHealth => 1,
            Self::EpochInfo => 2,
            Self::VoteAccounts => 3,
            Self::CurrentBlockProduction => 4,
            Self::CurrentLeaderSchedule => 5,
            Self::NextLeaderSchedule => 6,
            Self::InflationRewards => 7,
            Self::RewardEpochBlockProduction => 8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CommandId, ModeId, Transport};

    #[test]
    fn command_indices_and_modes_match_all_order() {
        for (index, command) in CommandId::ALL.into_iter().enumerate() {
            assert_eq!(command.index(), index);
        }

        assert_eq!(CommandId::RpcHealth.mode(), ModeId::Health);
        assert_eq!(CommandId::EpochInfo.mode(), ModeId::Consensus);
        assert_eq!(CommandId::CurrentLeaderSchedule.mode(), ModeId::Leadership);
        assert_eq!(CommandId::InflationRewards.mode(), ModeId::Economics);

        assert_eq!(CommandId::RpcHealth.transport(), Transport::Rpc);
        assert_eq!(CommandId::EpochInfo.transport(), Transport::Rpc);
        assert_eq!(CommandId::InflationRewards.transport(), Transport::Rpc);
        assert_eq!(CommandId::WsHealth.transport(), Transport::Websocket);
    }
}
