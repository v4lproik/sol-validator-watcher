//! Stable identifiers and source requirements for watcher collection modes.
//!
//! Commands declare transport, standard Solana RPC compatibility, and optional
//! provider-feature requirements here; routing code consumes those declarations.

use serde::{Deserialize, Serialize};

/// Number of collection modes in [`ModeId::ALL`].
pub const MODE_COUNT: usize = 4;
/// Number of typed commands in [`CommandId::ALL`].
pub const COMMAND_COUNT: usize = 15;

/// Transport required to execute a watcher command.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Transport {
    /// Solana JSON-RPC over HTTP.
    Rpc,
    /// Solana WebSocket connectivity.
    Websocket,
    /// Provider-specific HTTP API.
    ProviderApi,
}

impl Transport {
    /// Returns the stable label used in configuration and metrics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rpc => "rpc",
            Self::Websocket => "websocket",
            Self::ProviderApi => "provider-api",
        }
    }
}

/// Standard Solana JSON-RPC method profile declared by a connector.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SolanaRpcCompatibility {
    /// Methods needed for health, epoch, vote-account, and leader-schedule reads.
    Core,
    /// The core profile plus performance, block-production, and account reads.
    Extended,
}

impl SolanaRpcCompatibility {
    /// Returns the stable kebab-case configuration value.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Extended => "extended",
        }
    }

    /// Returns whether this declaration satisfies `required`.
    pub const fn supports(self, required: Self) -> bool {
        matches!(
            (self, required),
            (Self::Core, Self::Core) | (Self::Extended, _)
        )
    }
}

/// Provider-specific capability consumed by a typed watcher command.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderFeature {
    /// Retained prior-epoch data needed for historical rewards and production.
    HistoricalData,
    /// Bounded confirmed block rewards served through standard Solana `getBlock`.
    BlockFeeRewards,
    /// Settled per-validator rewards served by the Jito validator API.
    JitoMevRewards,
}

impl ProviderFeature {
    /// Returns the stable kebab-case configuration value.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HistoricalData => "historical-data",
            Self::BlockFeeRewards => "block-fee-rewards",
            Self::JitoMevRewards => "jito-mev-rewards",
        }
    }
}

/// Complete connector requirement for one typed command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModeSourceRequirement {
    /// Transport on which the command executes.
    pub transport: Transport,
    /// Minimum standard Solana RPC profile, if the command uses RPC.
    pub solana_rpc_compatibility: Option<SolanaRpcCompatibility>,
    /// Additional provider capability, if one is required.
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
const RPC_EXTENDED_BLOCK_FEE_SOURCE_REQUIREMENT: ModeSourceRequirement = ModeSourceRequirement {
    transport: Transport::Rpc,
    solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
    feature: Some(ProviderFeature::BlockFeeRewards),
};
const WEBSOCKET_SOURCE_REQUIREMENT: ModeSourceRequirement = ModeSourceRequirement {
    transport: Transport::Websocket,
    solana_rpc_compatibility: None,
    feature: None,
};
const PROVIDER_API_JITO_REWARDS_SOURCE_REQUIREMENT: ModeSourceRequirement = ModeSourceRequirement {
    transport: Transport::ProviderApi,
    solana_rpc_compatibility: None,
    feature: Some(ProviderFeature::JitoMevRewards),
};

/// Stable identifier for an independently reported watcher mode.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModeId {
    /// RPC and WebSocket dependency health.
    Health,
    /// Epoch and validator voting state.
    Consensus,
    /// Block production and leader scheduling.
    Leadership,
    /// Single-validator balances and prior-epoch rewards.
    Economics,
}

impl ModeId {
    /// Every mode in stable metric-label order.
    pub const ALL: [Self; MODE_COUNT] = [
        Self::Health,
        Self::Consensus,
        Self::Leadership,
        Self::Economics,
    ];

    /// Returns the stable metric and configuration label.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Health => "health",
            Self::Consensus => "consensus",
            Self::Leadership => "leadership",
            Self::Economics => "economics",
        }
    }
}

/// Stable identifier for a routed watcher operation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandId {
    /// Solana RPC health check.
    RpcHealth,
    /// WebSocket reachability check.
    WsHealth,
    /// Node-local identity, software, block height, and health.
    NodeStatus,
    /// Current epoch and slot information.
    EpochInfo,
    /// Current and delinquent vote accounts.
    VoteAccounts,
    /// Public gossip peers and their validator software metadata.
    ClusterNodes,
    /// Recent cluster slot-performance sample.
    RecentPerformanceSamples,
    /// Block production in the current epoch range.
    CurrentBlockProduction,
    /// Leader schedule covering the current slot.
    CurrentLeaderSchedule,
    /// Leader schedule for the next epoch boundary.
    NextLeaderSchedule,
    /// Identity and vote-account balances plus rent exemption.
    AccountBalances,
    /// Prior-epoch inflation rewards.
    InflationRewards,
    /// Block production retained for the prior reward epoch.
    RewardEpochBlockProduction,
    /// Settled prior-epoch Jito MEV reward for the economics validator.
    JitoMevRewards,
    /// Recent confirmed block-fee rewards for the economics validator.
    RecentBlockFeeRewards,
}

impl CommandId {
    /// Every command in stable routing-array order.
    pub const ALL: [Self; COMMAND_COUNT] = [
        Self::RpcHealth,
        Self::WsHealth,
        Self::NodeStatus,
        Self::EpochInfo,
        Self::VoteAccounts,
        Self::ClusterNodes,
        Self::RecentPerformanceSamples,
        Self::CurrentBlockProduction,
        Self::CurrentLeaderSchedule,
        Self::NextLeaderSchedule,
        Self::AccountBalances,
        Self::InflationRewards,
        Self::RewardEpochBlockProduction,
        Self::JitoMevRewards,
        Self::RecentBlockFeeRewards,
    ];

    /// Returns the stable metric and configuration label.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RpcHealth => "rpc-health",
            Self::WsHealth => "ws-health",
            Self::NodeStatus => "node-status",
            Self::EpochInfo => "epoch-info",
            Self::VoteAccounts => "vote-accounts",
            Self::ClusterNodes => "cluster-nodes",
            Self::RecentPerformanceSamples => "recent-performance-samples",
            Self::CurrentBlockProduction => "current-block-production",
            Self::CurrentLeaderSchedule => "current-leader-schedule",
            Self::NextLeaderSchedule => "next-leader-schedule",
            Self::AccountBalances => "account-balances",
            Self::InflationRewards => "inflation-rewards",
            Self::RewardEpochBlockProduction => "reward-epoch-block-production",
            Self::JitoMevRewards => "jito-mev-rewards",
            Self::RecentBlockFeeRewards => "recent-block-fee-rewards",
        }
    }

    /// Returns the mode that owns this command.
    pub const fn mode(self) -> ModeId {
        match self {
            Self::RpcHealth | Self::WsHealth | Self::NodeStatus => ModeId::Health,
            Self::EpochInfo
            | Self::VoteAccounts
            | Self::ClusterNodes
            | Self::RecentPerformanceSamples => ModeId::Consensus,
            Self::CurrentBlockProduction
            | Self::CurrentLeaderSchedule
            | Self::NextLeaderSchedule => ModeId::Leadership,
            Self::AccountBalances
            | Self::InflationRewards
            | Self::RewardEpochBlockProduction
            | Self::JitoMevRewards
            | Self::RecentBlockFeeRewards => ModeId::Economics,
        }
    }

    /// Returns the transport and capability contract for this command.
    pub const fn source_requirement(self) -> ModeSourceRequirement {
        match self {
            Self::RpcHealth
            | Self::NodeStatus
            | Self::EpochInfo
            | Self::VoteAccounts
            | Self::ClusterNodes
            | Self::CurrentLeaderSchedule
            | Self::NextLeaderSchedule => RPC_CORE_SOURCE_REQUIREMENT,
            Self::WsHealth => WEBSOCKET_SOURCE_REQUIREMENT,
            Self::RecentPerformanceSamples
            | Self::CurrentBlockProduction
            | Self::AccountBalances => RPC_EXTENDED_SOURCE_REQUIREMENT,
            Self::InflationRewards | Self::RewardEpochBlockProduction => {
                RPC_EXTENDED_HISTORICAL_SOURCE_REQUIREMENT
            }
            Self::JitoMevRewards => PROVIDER_API_JITO_REWARDS_SOURCE_REQUIREMENT,
            Self::RecentBlockFeeRewards => RPC_EXTENDED_BLOCK_FEE_SOURCE_REQUIREMENT,
        }
    }

    /// Returns the stable array index used by compiled command routes.
    pub const fn index(self) -> usize {
        match self {
            Self::RpcHealth => 0,
            Self::WsHealth => 1,
            Self::NodeStatus => 2,
            Self::EpochInfo => 3,
            Self::VoteAccounts => 4,
            Self::ClusterNodes => 5,
            Self::RecentPerformanceSamples => 6,
            Self::CurrentBlockProduction => 7,
            Self::CurrentLeaderSchedule => 8,
            Self::NextLeaderSchedule => 9,
            Self::AccountBalances => 10,
            Self::InflationRewards => 11,
            Self::RewardEpochBlockProduction => 12,
            Self::JitoMevRewards => 13,
            Self::RecentBlockFeeRewards => 14,
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
            CommandId::NodeStatus,
            CommandId::EpochInfo,
            CommandId::VoteAccounts,
            CommandId::ClusterNodes,
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
            CommandId::RecentBlockFeeRewards.source_requirement(),
            ModeSourceRequirement {
                transport: Transport::Rpc,
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
                feature: Some(ProviderFeature::BlockFeeRewards),
            }
        );
        assert_eq!(
            CommandId::JitoMevRewards.source_requirement(),
            ModeSourceRequirement {
                transport: Transport::ProviderApi,
                solana_rpc_compatibility: None,
                feature: Some(ProviderFeature::JitoMevRewards),
            }
        );
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
