use super::*;
#[test]
fn rejects_non_toml_config() -> Result<()> {
    let error = match Config::load(Path::new("config.yaml")) {
        Ok(_) => anyhow::bail!("non-TOML config should be rejected"),
        Err(error) => error,
    };

    assert!(error
        .to_string()
        .contains("config file config.yaml must use the .toml extension"));
    Ok(())
}

#[test]
fn rejects_transports_as_provider_features() -> Result<()> {
    for feature in ["rpc", "websocket", "historical-rpc"] {
        let raw = format!(
            r#"
network = "testnet"

[[connectors]]
name = "helius"
provider = "helius"
features = ["{feature}"]
rpc_url = "https://rpc.helius.example"
"#
        );
        let error = config_load_error("transport-provider-feature", &raw)?;

        assert!(format!("{error:#}").contains("unknown variant"));
    }
    Ok(())
}

#[test]
fn rejects_missing_provider_features() -> Result<()> {
    let error = config_load_error(
        "missing-provider-features",
        r#"
network = "testnet"

[[connectors]]
name = "helius"
provider = "helius"
rpc_url = "https://rpc.helius.example"
"#,
    )?;

    assert!(format!("{error:#}").contains("missing field `features`"));
    Ok(())
}

#[test]
fn rejects_invalid_provider_feature_declarations() -> Result<()> {
    let cases = [
        (
            ConnectorConfig {
                name: "primary".to_string(),
                rpc_url: Some("https://rpc.example".to_string()),
                ..ConnectorConfig::default()
            },
            "provider must not be empty",
        ),
        (
            ConnectorConfig {
                name: "primary".to_string(),
                provider: "solana".to_string(),
                features: vec![
                    ProviderFeature::HistoricalData,
                    ProviderFeature::HistoricalData,
                ],
                rpc_url: Some("https://rpc.example".to_string()),
                ..ConnectorConfig::default()
            },
            "contains duplicate provider feature \"historical-data\"",
        ),
    ];

    for (connector, expected) in cases {
        let mut config = Config {
            connectors: vec![connector],
            ..Config::default()
        };
        let Err(error) = config.validate() else {
            anyhow::bail!("invalid provider feature declaration should be rejected");
        };
        assert!(error.to_string().contains(expected), "{error:#}");
    }
    Ok(())
}

#[test]
fn validates_solana_rpc_compatibility_against_endpoint_and_features() -> Result<()> {
    let mut missing_compatibility = ConnectorConfig {
        name: "rpc".to_string(),
        provider: "solana-validator".to_string(),
        features: Vec::new(),
        rpc_url: Some("https://rpc.example".to_string()),
        ..ConnectorConfig::default()
    };
    let Err(error) = missing_compatibility.validate(0) else {
        anyhow::bail!("RPC endpoint without Solana RPC compatibility should fail");
    };
    assert!(error
        .to_string()
        .contains("solana_rpc_compatibility is required when rpc_url is configured"));

    let mut compatibility_without_endpoint = ConnectorConfig {
        name: "websocket".to_string(),
        provider: "solana-validator".to_string(),
        features: Vec::new(),
        solana_rpc_compatibility: Some(SolanaRpcCompatibility::Core),
        ws_url: Some("wss://ws.example".to_string()),
        ..ConnectorConfig::default()
    };
    let Err(error) = compatibility_without_endpoint.validate(0) else {
        anyhow::bail!("Solana RPC compatibility without RPC endpoint should fail");
    };
    assert!(error
        .to_string()
        .contains("solana_rpc_compatibility requires a configured rpc_url"));

    let mut history_on_core = ConnectorConfig {
        name: "core".to_string(),
        provider: "solana-validator".to_string(),
        features: vec![ProviderFeature::HistoricalData],
        solana_rpc_compatibility: Some(SolanaRpcCompatibility::Core),
        rpc_url: Some("https://rpc.example".to_string()),
        ..ConnectorConfig::default()
    };
    let Err(error) = history_on_core.validate(0) else {
        anyhow::bail!("historical data on core Solana RPC should fail");
    };
    assert!(error.to_string().contains(
        "features entry \"historical-data\" requires solana_rpc_compatibility = \"extended\""
    ));
    Ok(())
}

#[test]
fn provider_features_deserialize_and_export_stable_labels() -> Result<()> {
    let connector: ConnectorConfig = toml::from_str(
        r#"
name = "test"
provider = "helius"
features = ["historical-data"]
solana_rpc_compatibility = "extended"
rpc_url = "https://rpc.example"
ws_url = "wss://rpc.example"
"#,
    )?;

    assert_eq!(connector.provider, "helius");
    assert_eq!(connector.features, [ProviderFeature::HistoricalData]);
    assert_eq!(
        connector.solana_rpc_compatibility,
        Some(SolanaRpcCompatibility::Extended)
    );
    assert_eq!(
        connector
            .features
            .iter()
            .map(|feature| feature.as_str())
            .collect::<Vec<_>>(),
        ["historical-data"]
    );
    Ok(())
}

#[test]
fn rejects_removed_root_and_unknown_connector_fields() -> Result<()> {
    for (field_name, setting) in [
        ("validator_mode", "validator_mode = \"rpc\""),
        (
            "validator_websocket_enabled",
            "validator_websocket_enabled = true",
        ),
    ] {
        let error = config_load_error(
            field_name,
            &format!(
                r#"
network = "testnet"
{setting}

[[connectors]]
name = "primary"
provider = "solana"
features = []
rpc_url = "https://rpc.example"
"#
            ),
        )?;
        let message = format!("{error:#}");
        assert!(
            message.contains(&format!("unknown field `{field_name}`")),
            "{message}"
        );
    }

    let connector_error = config_load_error(
        "unknown-connector-field",
        r#"
network = "testnet"

[[connectors]]
name = "primary"
provider = "solana"
features = []
rpc_url = "https://rpc.example"
transport_mode = "poll-http"
"#,
    )?;
    let connector_message = format!("{connector_error:#}");
    assert!(
        connector_message.contains("unknown field `transport_mode`"),
        "{connector_message}"
    );

    Ok(())
}

#[test]
fn rejects_missing_connector_list() -> Result<()> {
    let mut config = Config::default();

    match config.validate() {
        Ok(()) => anyhow::bail!("missing connectors should be rejected"),
        Err(error) => assert!(error
            .to_string()
            .contains("at least one connector is required")),
    }
    Ok(())
}

#[test]
fn rejects_ws_only_connector_list() -> Result<()> {
    let mut config = Config {
        connectors: vec![ConnectorConfig {
            name: "ws-only".to_string(),
            provider: "helius".to_string(),
            features: Vec::new(),
            ws_url: Some("wss://ws.helius.example".to_string()),
            ..ConnectorConfig::default()
        }],
        ..Config::default()
    };

    match config.validate() {
        Ok(()) => anyhow::bail!("a websocket-only list should be rejected"),
        Err(error) => assert!(error
            .to_string()
            .contains("at least one connector with rpc_url is required")),
    }
    Ok(())
}

#[test]
fn rejects_duplicate_connector_names() -> Result<()> {
    let mut config = Config {
        connectors: vec![rpc_connector("primary"), rpc_connector("primary")],
        ..Config::default()
    };

    match config.validate() {
        Ok(()) => anyhow::bail!("duplicate connector names should be rejected"),
        Err(error) => assert!(error
            .to_string()
            .contains("duplicate connector name \"primary\"")),
    }
    Ok(())
}

#[test]
fn rejects_invalid_ws_scheme() -> Result<()> {
    let mut connector = rpc_connector("primary");
    connector.ws_url = Some("https://api.mainnet-beta.solana.com".to_string());
    let mut config = Config {
        connectors: vec![connector],
        ..Config::default()
    };

    match config.validate() {
        Ok(()) => anyhow::bail!("invalid websocket URL should be rejected"),
        Err(error) => assert!(error
            .to_string()
            .contains("ws_url must start with ws:// or wss://")),
    }
    Ok(())
}

#[test]
fn trims_blank_ws_url() -> Result<()> {
    let mut connector = rpc_connector("primary");
    connector.ws_url = Some("   ".to_string());
    let mut config = Config {
        connectors: vec![connector],
        modes: modes_for("primary", None, false),
        ..Config::default()
    };

    config.validate()?;

    assert!(config.connectors[0].ws_url.is_none());
    Ok(())
}

#[test]
fn rejects_unknown_and_duplicate_command_route_connectors() -> Result<()> {
    let mut unknown = Config {
        connectors: vec![rpc_connector("primary")],
        modes: modes_for("primary", None, false),
        ..Config::default()
    };
    unknown.modes.consensus.commands.epoch_info = vec!["missing".to_string()];
    let Err(error) = unknown.validate() else {
        anyhow::bail!("unknown route connector should be rejected");
    };
    assert!(error
        .to_string()
        .contains("references unknown connector \"missing\""));

    let mut duplicate = Config {
        connectors: vec![rpc_connector("primary")],
        modes: modes_for("primary", None, false),
        ..Config::default()
    };
    duplicate.modes.consensus.commands.epoch_info =
        vec!["primary".to_string(), "primary".to_string()];
    let Err(error) = duplicate.validate() else {
        anyhow::bail!("duplicate route connector should be rejected");
    };
    assert!(error
        .to_string()
        .contains("contains duplicate connector name \"primary\""));
    Ok(())
}

#[test]
fn rejects_rpc_command_routed_to_websocket_only_connector() -> Result<()> {
    let mut config = Config {
        connectors: vec![
            rpc_connector("primary"),
            ConnectorConfig {
                name: "ws-only".to_string(),
                provider: "helius".to_string(),
                features: Vec::new(),
                ws_url: Some("wss://ws.example".to_string()),
                ..ConnectorConfig::default()
            },
        ],
        modes: modes_for("primary", Some("ws-only"), false),
        ..Config::default()
    };
    config.modes.leadership.commands.current_block_production = vec!["ws-only".to_string()];

    let Err(error) = config.validate() else {
        anyhow::bail!("RPC command without RPC transport should be rejected");
    };
    assert!(error.to_string().contains("has no configured RPC endpoint"));
    Ok(())
}

#[test]
fn rejects_extended_rpc_command_routed_to_core_endpoint() -> Result<()> {
    let mut connector = rpc_connector("core");
    connector.features.clear();
    connector.solana_rpc_compatibility = Some(SolanaRpcCompatibility::Core);
    let mut config = Config {
        connectors: vec![connector],
        modes: modes_for("core", None, false),
        ..Config::default()
    };

    let Err(error) = config.validate() else {
        anyhow::bail!("extended RPC command on core endpoint should fail");
    };
    let message = error.to_string();
    assert!(
        message.contains("modes.consensus.commands.recent_performance_samples"),
        "{message}"
    );
    assert!(message.contains("connector \"core\""), "{message}");
    assert!(
        message.contains("requires solana_rpc_compatibility = \"extended\""),
        "{message}"
    );
    assert!(message.contains("declares \"core\""), "{message}");
    Ok(())
}

#[test]
fn permits_core_rpc_endpoint_when_extended_rpc_commands_are_omitted() -> Result<()> {
    let mut connector = rpc_connector("core");
    connector.features.clear();
    connector.solana_rpc_compatibility = Some(SolanaRpcCompatibility::Core);
    let mut modes = modes_for("core", None, false);
    modes.consensus.commands.recent_performance_samples.clear();
    modes.leadership.commands.current_block_production.clear();
    let mut config = Config {
        connectors: vec![connector],
        modes,
        ..Config::default()
    };

    config.validate()?;
    Ok(())
}

#[test]
fn rejects_economics_route_without_historical_data_feature() -> Result<()> {
    let mut connector = rpc_connector("primary");
    connector.features = Vec::new();
    let mut config = Config {
        connectors: vec![connector],
        modes: modes_for("primary", None, true),
        ..Config::default()
    };
    config.modes.economics.validator = Some(WatchedValidatorConfig {
        identity: None,
        vote_account: Some("vote-1".to_string()),
        labels: vec![],
    });

    let Err(error) = config.validate() else {
        anyhow::bail!("economics route without historical data should be rejected");
    };
    assert!(error
        .to_string()
        .contains("requires provider feature \"historical-data\""));
    Ok(())
}

#[test]
fn permits_economics_balance_route_without_historical_data_feature() -> Result<()> {
    let mut standard = rpc_connector("standard");
    standard.features = Vec::new();
    let historical = rpc_connector("historical");
    let mut config = Config {
        connectors: vec![standard, historical],
        modes: modes_for("historical", None, true),
        ..Config::default()
    };
    config.modes.economics.commands.account_balances = vec!["standard".to_string()];
    config.modes.economics.validator = Some(WatchedValidatorConfig {
        identity: Some("id-1".to_string()),
        vote_account: Some("vote-1".to_string()),
        labels: vec![],
    });

    config.validate()
}

#[test]
fn rejects_economics_routes_while_mode_is_disabled() -> Result<()> {
    let mut config = Config {
        connectors: vec![rpc_connector("primary")],
        modes: modes_for("primary", None, false),
        ..Config::default()
    };
    config.modes.economics.commands.inflation_rewards = vec!["primary".to_string()];

    let Err(error) = config.validate() else {
        anyhow::bail!("disabled economics routes should be rejected");
    };
    assert!(error
        .to_string()
        .contains("requires modes.economics.enabled = true"));
    Ok(())
}

#[test]
fn economics_single_validator_is_merged_into_watched_validators() -> Result<()> {
    let mut config = Config {
        connectors: vec![rpc_connector("primary")],
        modes: modes_for("primary", None, true),
        watched_validators: vec![WatchedValidatorConfig {
            identity: Some("11111111111111111111111111111111".to_string()),
            vote_account: None,
            labels: vec!["region:sbg".to_string()],
        }],
        ..Config::default()
    };
    config.modes.economics.validator = Some(WatchedValidatorConfig {
        identity: Some("11111111111111111111111111111111".to_string()),
        vote_account: None,
        labels: vec!["team:ops".to_string()],
    });

    config.validate()?;

    assert_eq!(config.watched_validators.len(), 1);
    assert!(config.watched_validators[0]
        .labels
        .contains(&"region:sbg".to_string()));
    assert!(config.watched_validators[0]
        .labels
        .contains(&"team:ops".to_string()));
    assert!(config.watched_validators[0]
        .labels
        .contains(&ECONOMICS_TARGET_LABEL.to_string()));
    Ok(())
}

#[test]
fn rejects_economics_validator_without_single_validator_mode() -> Result<()> {
    let mut config = Config {
        connectors: vec![rpc_connector("primary")],
        modes: ModesConfig {
            economics: EconomicsModeConfig {
                enabled: false,
                validator: Some(WatchedValidatorConfig {
                    identity: None,
                    vote_account: Some("vote-1".to_string()),
                    labels: vec![],
                }),
                ..EconomicsModeConfig::default()
            },
            ..modes_for("primary", None, false)
        },
        ..Config::default()
    };

    match config.validate() {
        Ok(()) => anyhow::bail!("economics validator should be rejected"),
        Err(error) => assert!(error
            .to_string()
            .contains("modes.economics.validator requires modes.economics.enabled = true")),
    }
    Ok(())
}

#[test]
fn rejects_missing_economics_validator_in_single_validator_mode() -> Result<()> {
    let mut config = Config {
        connectors: vec![rpc_connector("primary")],
        modes: ModesConfig {
            economics: EconomicsModeConfig {
                enabled: true,
                validator: None,
                commands: EconomicsCommands {
                    account_balances: vec!["primary".to_string()],
                    inflation_rewards: vec!["primary".to_string()],
                    reward_epoch_block_production: vec!["primary".to_string()],
                },
            },
            ..modes_for("primary", None, false)
        },
        ..Config::default()
    };

    match config.validate() {
        Ok(()) => anyhow::bail!("missing economics validator should be rejected"),
        Err(error) => assert!(error
            .to_string()
            .contains("modes.economics.enabled requires modes.economics.validator")),
    }
    Ok(())
}
