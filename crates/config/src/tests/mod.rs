use super::{
    expand_env_vars_with, Config, ConnectorConfig, ConsensusCommands, ConsensusModeConfig,
    EconomicsCommands, EconomicsModeConfig, HealthCommands, HealthModeConfig, LeadershipCommands,
    LeadershipModeConfig, ModesConfig, WatchedValidatorConfig, ECONOMICS_TARGET_LABEL,
};
use anyhow::{Context, Result};
use sol_validator_watcher_domain::{
    CommandId, ModeSourceRequirement, ProviderFeature, SolanaRpcCompatibility,
};
use std::{
    fs,
    net::{IpAddr, Ipv4Addr},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

mod env;
mod load;
mod validation;

fn config_load_error(prefix: &str, raw: &str) -> Result<anyhow::Error> {
    let test_dir = unique_test_dir(prefix);
    fs::create_dir_all(&test_dir)
        .with_context(|| format!("failed to create {}", test_dir.display()))?;
    let config_path = test_dir.join("watcher.toml");
    fs::write(&config_path, raw)
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    let result = Config::load(&config_path);
    fs::remove_dir_all(&test_dir)
        .with_context(|| format!("failed to remove directory {}", test_dir.display()))?;

    match result {
        Ok(_) => anyhow::bail!("configuration should be rejected"),
        Err(error) => Ok(error),
    }
}

fn rpc_connector(name: &str) -> ConnectorConfig {
    ConnectorConfig {
        name: name.to_string(),
        provider: "solana".to_string(),
        features: vec![ProviderFeature::HistoricalData],
        solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
        rpc: Some(crate::RpcTransportConfig::new(
            "https://api.mainnet-beta.solana.com".to_string(),
        )),
        ..ConnectorConfig::default()
    }
}

fn modes_for(rpc: &str, ws: Option<&str>, economics_enabled: bool) -> ModesConfig {
    let rpc_route = || vec![rpc.to_string()];
    ModesConfig {
        health: HealthModeConfig {
            commands: HealthCommands {
                rpc_health: rpc_route(),
                ws_health: ws.into_iter().map(str::to_string).collect(),
                node_status: rpc_route(),
            },
        },
        consensus: ConsensusModeConfig {
            commands: ConsensusCommands {
                epoch_info: rpc_route(),
                vote_accounts: rpc_route(),
                cluster_nodes: rpc_route(),
                recent_performance_samples: rpc_route(),
            },
        },
        leadership: LeadershipModeConfig {
            commands: LeadershipCommands {
                current_block_production: rpc_route(),
                current_leader_schedule: rpc_route(),
                next_leader_schedule: rpc_route(),
            },
        },
        economics: EconomicsModeConfig {
            enabled: economics_enabled,
            validator: None,
            reserve_wallets: vec![],
            commands: EconomicsCommands {
                account_balances: economics_enabled.then(rpc_route).unwrap_or_default(),
                reserve_wallet_balances: vec![],
                inflation_rewards: economics_enabled.then(rpc_route).unwrap_or_default(),
                reward_epoch_block_production: economics_enabled
                    .then(rpc_route)
                    .unwrap_or_default(),
                recent_block_fee_rewards: vec![],
                jito_mev_rewards: vec![],
                trillium_mev_capture: vec![],
            },
        },
    }
}

fn unique_test_dir(prefix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    PathBuf::from("target").join(format!(
        "sol-validator-watcher-config-{}-{}-{}",
        prefix,
        std::process::id(),
        nonce
    ))
}
