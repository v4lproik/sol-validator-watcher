use super::*;
#[test]
fn loads_toml_and_preserves_explicit_command_route_order() -> Result<()> {
    let test_dir = unique_test_dir("ordered-connectors");
    fs::create_dir_all(&test_dir)
        .with_context(|| format!("failed to create {}", test_dir.display()))?;
    let config_path = test_dir.join("watcher.toml");
    fs::write(
        &config_path,
        r#"
network = "testnet"

[[connectors]]
name = "local-validator"
provider = "solana-validator"
features = []
solana_rpc_compatibility = "core"
rpc_url = "http://127.0.0.1:8899"

[[connectors]]
name = "helius-rpc"
provider = "helius"
features = []
solana_rpc_compatibility = "extended"
rpc_url = "https://rpc.helius.example"

[[connectors]]
name = "helius-ws"
provider = "helius"
features = []
ws_url = "wss://ws.helius.example"

[[watched_validators]]
identity = "11111111111111111111111111111111"
labels = ["vc:validator-1", "region:sbg"]

[modes.health.commands]
rpc_health = ["local-validator", "helius-rpc"]
ws_health = ["helius-ws"]

[modes.consensus.commands]
epoch_info = ["local-validator", "helius-rpc"]
vote_accounts = ["local-validator", "helius-rpc"]
recent_performance_samples = ["helius-rpc"]

[modes.leadership.commands]
current_block_production = ["helius-rpc"]
current_leader_schedule = ["local-validator", "helius-rpc"]
next_leader_schedule = ["local-validator", "helius-rpc"]
"#,
    )
    .with_context(|| format!("failed to write {}", config_path.display()))?;

    let config = Config::load(&config_path)?;

    assert_eq!(config.network, "testnet");
    assert_eq!(config.connectors.len(), 3);
    assert_eq!(config.connectors[0].name, "local-validator");
    assert_eq!(config.connectors[0].provider, "solana-validator");
    assert!(config.connectors[0].features.is_empty());
    assert_eq!(
        config.connectors[0].solana_rpc_compatibility,
        Some(SolanaRpcCompatibility::Core)
    );
    assert_eq!(config.connectors[1].name, "helius-rpc");
    assert_eq!(config.connectors[1].provider, "helius");
    assert_eq!(
        config.connectors[1].solana_rpc_compatibility,
        Some(SolanaRpcCompatibility::Extended)
    );
    assert_eq!(config.connectors[2].name, "helius-ws");
    assert!(config.connectors[2].features.is_empty());
    assert_eq!(config.watched_validators.len(), 1);
    assert_eq!(
        config.modes.leadership.commands.current_block_production,
        ["helius-rpc"]
    );
    assert_eq!(config.modes.health.commands.ws_health, ["helius-ws"]);
    assert_eq!(
        config.modes.consensus.commands.recent_performance_samples,
        ["helius-rpc"]
    );

    fs::remove_dir_all(&test_dir)
        .with_context(|| format!("failed to remove directory {}", test_dir.display()))?;
    Ok(())
}

#[test]
fn loads_every_committed_profile() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cases = [
        (
            "config/local/validator.toml",
            "mainnet-beta",
            "local-validator",
            "solana-validator",
            SolanaRpcCompatibility::Core,
            false,
            false,
        ),
        (
            "config/compose/validator.toml",
            "external-validator",
            "external-validator",
            "solana-validator",
            SolanaRpcCompatibility::Core,
            false,
            false,
        ),
        (
            "config/compose/surfpool.toml",
            "surfpool-localnet",
            "surfpool",
            "surfpool",
            SolanaRpcCompatibility::Extended,
            true,
            false,
        ),
        (
            "config/remote/devnet.toml",
            "devnet",
            "helius-devnet",
            "helius",
            SolanaRpcCompatibility::Extended,
            true,
            true,
        ),
        (
            "config/remote/mainnet.toml",
            "mainnet-beta",
            "mainnet-rpc",
            "helius",
            SolanaRpcCompatibility::Extended,
            true,
            true,
        ),
    ];

    for (
        relative_path,
        network,
        connector_name,
        provider,
        solana_rpc_compatibility,
        has_ws,
        has_history,
    ) in cases
    {
        let path = root.join(relative_path);
        let config = Config::load_with(&path, |raw| {
            expand_env_vars_with(raw, |key| {
                let value = match key {
                    "SOL_DEVNET_RPC_URL" => Some(
                        "https://devnet.example/custom/rpc?api-key=test-key&region=us".to_string(),
                    ),
                    "SOL_DEVNET_WS_URL" => Some(
                        "wss://devnet.example/custom/ws?api-key=test-key&region=us".to_string(),
                    ),
                    "SOL_MAINNET_RPC_URL" => Some("http://127.0.0.1:8899".to_string()),
                    "SOL_MAINNET_WS_URL" => Some("ws://127.0.0.1:8900".to_string()),
                    _ => None,
                };
                Ok(value)
            })
        })
        .with_context(|| format!("failed to load committed profile {}", path.display()))?;

        assert_eq!(config.network, network, "profile {relative_path}");
        assert_eq!(config.connectors.len(), 1, "profile {relative_path}");
        assert_eq!(
            config.connectors[0].name, connector_name,
            "profile {relative_path}"
        );
        assert_eq!(
            config.connectors[0].provider, provider,
            "profile {relative_path}"
        );
        assert!(config.connectors[0].has_rpc(), "profile {relative_path}");
        assert_eq!(
            config.connectors[0].solana_rpc_compatibility,
            Some(solana_rpc_compatibility),
            "profile {relative_path}"
        );
        assert_eq!(
            config.connectors[0].has_ws(),
            has_ws,
            "profile {relative_path}"
        );
        assert_eq!(
            config.connectors[0].supports(ProviderFeature::HistoricalData),
            has_history,
            "profile {relative_path}"
        );

        if relative_path == "config/remote/devnet.toml" {
            assert_eq!(
                config.connectors[0].rpc_url.as_deref(),
                Some("https://devnet.example/custom/rpc?api-key=test-key&region=us")
            );
            assert_eq!(
                config.connectors[0].ws_url.as_deref(),
                Some("wss://devnet.example/custom/ws?api-key=test-key&region=us")
            );
            assert!(config.modes.economics.enabled);
            assert_eq!(
                config.modes.route(CommandId::InflationRewards),
                ["helius-devnet"]
            );
            assert_eq!(
                CommandId::InflationRewards.source_requirement(),
                ModeSourceRequirement {
                    transport: Transport::Rpc,
                    solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
                    feature: Some(ProviderFeature::HistoricalData),
                }
            );
        }
    }

    Ok(())
}
