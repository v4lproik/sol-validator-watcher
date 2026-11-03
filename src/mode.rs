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
    pub feature: Option<ProviderFeature>,
}

const HEALTH_SOURCE_REQUIREMENTS: [ModeSourceRequirement; 2] = [
    ModeSourceRequirement {
        transport: Transport::Rpc,
        feature: None,
    },
    ModeSourceRequirement {
        transport: Transport::Websocket,
        feature: None,
    },
];
const RPC_SOURCE_REQUIREMENTS: [ModeSourceRequirement; 1] = [ModeSourceRequirement {
    transport: Transport::Rpc,
    feature: None,
}];
const ECONOMICS_SOURCE_REQUIREMENTS: [ModeSourceRequirement; 2] = [
    ModeSourceRequirement {
        transport: Transport::Rpc,
        feature: None,
    },
    ModeSourceRequirement {
        transport: Transport::Rpc,
        feature: Some(ProviderFeature::HistoricalData),
    },
];

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

    pub const fn source_requirements(self) -> &'static [ModeSourceRequirement] {
        match self {
            Self::Health => &HEALTH_SOURCE_REQUIREMENTS,
            Self::Consensus | Self::Leadership => &RPC_SOURCE_REQUIREMENTS,
            Self::Economics => &ECONOMICS_SOURCE_REQUIREMENTS,
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
            Self::RpcHealth => ModeId::Health.source_requirements()[0],
            Self::WsHealth => ModeId::Health.source_requirements()[1],
            Self::EpochInfo | Self::VoteAccounts | Self::RecentPerformanceSamples => {
                ModeId::Consensus.source_requirements()[0]
            }
            Self::CurrentBlockProduction
            | Self::CurrentLeaderSchedule
            | Self::NextLeaderSchedule => ModeId::Leadership.source_requirements()[0],
            Self::AccountBalances => ModeId::Economics.source_requirements()[0],
            Self::InflationRewards | Self::RewardEpochBlockProduction => {
                ModeId::Economics.source_requirements()[1]
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
    use super::{CommandId, ModeId, ModeSourceRequirement, ProviderFeature, Transport};

    #[test]
    fn command_indices_and_modes_match_all_order() {
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
        assert_eq!(CommandId::InflationRewards.mode(), ModeId::Economics);

        assert_eq!(
            ModeId::Health.source_requirements(),
            [
                ModeSourceRequirement {
                    transport: Transport::Rpc,
                    feature: None,
                },
                ModeSourceRequirement {
                    transport: Transport::Websocket,
                    feature: None,
                },
            ]
        );
        assert_eq!(
            ModeId::Consensus.source_requirements(),
            [ModeSourceRequirement {
                transport: Transport::Rpc,
                feature: None,
            }]
        );
        assert_eq!(
            ModeId::Leadership.source_requirements(),
            [ModeSourceRequirement {
                transport: Transport::Rpc,
                feature: None,
            }]
        );
        assert_eq!(
            ModeId::Economics.source_requirements(),
            [
                ModeSourceRequirement {
                    transport: Transport::Rpc,
                    feature: None,
                },
                ModeSourceRequirement {
                    transport: Transport::Rpc,
                    feature: Some(ProviderFeature::HistoricalData),
                },
            ]
        );
        for command in CommandId::ALL {
            assert!(
                command
                    .mode()
                    .source_requirements()
                    .contains(&command.source_requirement()),
                "{} must use a source combination declared by mode {}",
                command.as_str(),
                command.mode().as_str()
            );
        }
    }
}
