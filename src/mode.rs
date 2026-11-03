use serde::{Deserialize, Serialize};

pub const MODE_COUNT: usize = 4;
pub const COMMAND_COUNT: usize = 11;

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
#[serde(rename_all = "kebab-case")]
pub enum SolanaRpcCompatibility {
    Core,
    Extended,
}

impl SolanaRpcCompatibility {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Extended => "extended",
        }
    }

    pub const fn supports(self, required: Self) -> bool {
        matches!(
            (self, required),
            (Self::Core, Self::Core) | (Self::Extended, _)
        )
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderFeature {
    HistoricalData,
}

impl ProviderFeature {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HistoricalData => "historical-data",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModeSourceRequirement {
    pub transport: Transport,
    pub solana_rpc_compatibility: Option<SolanaRpcCompatibility>,
    pub feature: Option<ProviderFeature>,
}

const RPC_CORE_SOURCE_REQUIREMENT: ModeSourceRequirement = ModeSourceRequirement {
    transport: Transport::Rpc,
    solana_rpc_compatibility: Some(SolanaRpcCompatibility::Core),
    feature: None,
};
const RPC_EXTENDED_SOURCE_REQUIREMENT: ModeSourceRequirement = ModeSourceRequirement {
    transport: Transport::Rpc,
    solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
    feature: None,
};
const RPC_EXTENDED_HISTORICAL_SOURCE_REQUIREMENT: ModeSourceRequirement = ModeSourceRequirement {
    transport: Transport::Rpc,
    solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
    feature: Some(ProviderFeature::HistoricalData),
};
const WEBSOCKET_SOURCE_REQUIREMENT: ModeSourceRequirement = ModeSourceRequirement {
    transport: Transport::Websocket,
    solana_rpc_compatibility: None,
    feature: None,
};

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
    RecentPerformanceSamples,
    CurrentBlockProduction,
    CurrentLeaderSchedule,
    NextLeaderSchedule,
    AccountBalances,
    InflationRewards,
    RewardEpochBlockProduction,
}

impl CommandId {
    pub const ALL: [Self; COMMAND_COUNT] = [
        Self::RpcHealth,
        Self::WsHealth,
        Self::EpochInfo,
        Self::VoteAccounts,
        Self::RecentPerformanceSamples,
        Self::CurrentBlockProduction,
        Self::CurrentLeaderSchedule,
        Self::NextLeaderSchedule,
        Self::AccountBalances,
        Self::InflationRewards,
        Self::RewardEpochBlockProduction,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RpcHealth => "rpc-health",
            Self::WsHealth => "ws-health",
            Self::EpochInfo => "epoch-info",
            Self::VoteAccounts => "vote-accounts",
            Self::RecentPerformanceSamples => "recent-performance-samples",
            Self::CurrentBlockProduction => "current-block-production",
            Self::CurrentLeaderSchedule => "current-leader-schedule",
            Self::NextLeaderSchedule => "next-leader-schedule",
            Self::AccountBalances => "account-balances",
            Self::InflationRewards => "inflation-rewards",
            Self::RewardEpochBlockProduction => "reward-epoch-block-production",
        }
    }

    pub const fn mode(self) -> ModeId {
        match self {
            Self::RpcHealth | Self::WsHealth => ModeId::Health,
            Self::EpochInfo | Self::VoteAccounts | Self::RecentPerformanceSamples => {
                ModeId::Consensus
            }
            Self::CurrentBlockProduction
            | Self::CurrentLeaderSchedule
            | Self::NextLeaderSchedule => ModeId::Leadership,
            Self::AccountBalances | Self::InflationRewards | Self::RewardEpochBlockProduction => {
                ModeId::Economics
            }
        }
    }

    pub const fn source_requirement(self) -> ModeSourceRequirement {
        match self {
            Self::RpcHealth
            | Self::EpochInfo
            | Self::VoteAccounts
            | Self::CurrentLeaderSchedule
            | Self::NextLeaderSchedule => RPC_CORE_SOURCE_REQUIREMENT,
            Self::WsHealth => WEBSOCKET_SOURCE_REQUIREMENT,
            Self::RecentPerformanceSamples
            | Self::CurrentBlockProduction
            | Self::AccountBalances => RPC_EXTENDED_SOURCE_REQUIREMENT,
            Self::InflationRewards | Self::RewardEpochBlockProduction => {
                RPC_EXTENDED_HISTORICAL_SOURCE_REQUIREMENT
            }
        }
    }

    pub const fn index(self) -> usize {
        match self {
            Self::RpcHealth => 0,
            Self::WsHealth => 1,
            Self::EpochInfo => 2,
            Self::VoteAccounts => 3,
            Self::RecentPerformanceSamples => 4,
            Self::CurrentBlockProduction => 5,
            Self::CurrentLeaderSchedule => 6,
            Self::NextLeaderSchedule => 7,
            Self::AccountBalances => 8,
            Self::InflationRewards => 9,
            Self::RewardEpochBlockProduction => 10,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CommandId, ModeId, ModeSourceRequirement, ProviderFeature, SolanaRpcCompatibility,
        Transport,
    };

    #[test]
    fn command_indices_modes_and_capabilities_match_all_order() {
        for (index, command) in CommandId::ALL.into_iter().enumerate() {
            assert_eq!(command.index(), index);
        }

        assert_eq!(CommandId::RpcHealth.mode(), ModeId::Health);
        assert_eq!(
            CommandId::RecentPerformanceSamples.mode(),
            ModeId::Consensus
        );
        assert_eq!(CommandId::CurrentLeaderSchedule.mode(), ModeId::Leadership);
        assert_eq!(CommandId::AccountBalances.mode(), ModeId::Economics);

        let core_rpc = ModeSourceRequirement {
            transport: Transport::Rpc,
            solana_rpc_compatibility: Some(SolanaRpcCompatibility::Core),
            feature: None,
        };
        let extended_rpc = ModeSourceRequirement {
            transport: Transport::Rpc,
            solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
            feature: None,
        };
        let historical_rpc = ModeSourceRequirement {
            feature: Some(ProviderFeature::HistoricalData),
            ..extended_rpc
        };

        for command in [
            CommandId::RpcHealth,
            CommandId::EpochInfo,
            CommandId::VoteAccounts,
            CommandId::CurrentLeaderSchedule,
            CommandId::NextLeaderSchedule,
        ] {
            assert_eq!(command.source_requirement(), core_rpc);
        }
        for command in [
            CommandId::RecentPerformanceSamples,
            CommandId::CurrentBlockProduction,
            CommandId::AccountBalances,
        ] {
            assert_eq!(command.source_requirement(), extended_rpc);
        }
        for command in [
            CommandId::InflationRewards,
            CommandId::RewardEpochBlockProduction,
        ] {
            assert_eq!(command.source_requirement(), historical_rpc);
        }
        assert_eq!(
            CommandId::WsHealth.source_requirement(),
            ModeSourceRequirement {
                transport: Transport::Websocket,
                solana_rpc_compatibility: None,
                feature: None,
            }
        );

        assert!(SolanaRpcCompatibility::Extended.supports(SolanaRpcCompatibility::Core));
        assert!(SolanaRpcCompatibility::Extended.supports(SolanaRpcCompatibility::Extended));
        assert!(SolanaRpcCompatibility::Core.supports(SolanaRpcCompatibility::Core));
        assert!(!SolanaRpcCompatibility::Core.supports(SolanaRpcCompatibility::Extended));
    }
}
