use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const COMMAND_COUNT: usize = 9;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    HealthRpc,
    HealthWs,
    EpochInfo,
    VoteAccounts,
    CurrentBlockProduction,
    LeaderSchedule,
    InflationRewards,
    RewardEpochBlockProduction,
}

impl Capability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HealthRpc => "health-rpc",
            Self::HealthWs => "health-ws",
            Self::EpochInfo => "epoch-info",
            Self::VoteAccounts => "vote-accounts",
            Self::CurrentBlockProduction => "current-block-production",
            Self::LeaderSchedule => "leader-schedule",
            Self::InflationRewards => "inflation-rewards",
            Self::RewardEpochBlockProduction => "reward-epoch-block-production",
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

    pub const fn capability(self) -> Capability {
        match self {
            Self::RpcHealth => Capability::HealthRpc,
            Self::WsHealth => Capability::HealthWs,
            Self::EpochInfo => Capability::EpochInfo,
            Self::VoteAccounts => Capability::VoteAccounts,
            Self::CurrentBlockProduction => Capability::CurrentBlockProduction,
            Self::CurrentLeaderSchedule | Self::NextLeaderSchedule => Capability::LeaderSchedule,
            Self::InflationRewards => Capability::InflationRewards,
            Self::RewardEpochBlockProduction => Capability::RewardEpochBlockProduction,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderDriver {
    StandardRpc,
    EnhancedRpc,
    WebsocketProbe,
}

impl ProviderDriver {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StandardRpc => "standard-rpc",
            Self::EnhancedRpc => "enhanced-rpc",
            Self::WebsocketProbe => "websocket-probe",
        }
    }

    pub const fn supports(self, capability: Capability) -> bool {
        match self {
            Self::StandardRpc | Self::EnhancedRpc => matches!(
                capability,
                Capability::HealthRpc
                    | Capability::EpochInfo
                    | Capability::VoteAccounts
                    | Capability::CurrentBlockProduction
                    | Capability::LeaderSchedule
                    | Capability::InflationRewards
                    | Capability::RewardEpochBlockProduction
            ),
            Self::WebsocketProbe => matches!(capability, Capability::HealthWs),
        }
    }
}

pub type CapabilityPriorityOverrides = BTreeMap<Capability, i32>;
pub type CommandPriorityOverrides = BTreeMap<CommandId, i32>;

#[cfg(test)]
mod tests {
    use super::CommandId;

    #[test]
    fn command_indices_match_all_order() {
        for (index, command) in CommandId::ALL.into_iter().enumerate() {
            assert_eq!(command.index(), index);
        }
    }
}
