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
rpc = {{ url = "https://rpc.helius.example" }}
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
rpc = { url = "https://rpc.helius.example" }
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
                rpc: Some(crate::config::RpcTransportConfig::new(
                    "https://rpc.example".to_string(),
                )),
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
                rpc: Some(crate::config::RpcTransportConfig::new(
                    "https://rpc.example".to_string(),
                )),
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
        rpc: Some(crate::config::RpcTransportConfig::new(
            "https://rpc.example".to_string(),
        )),
        ..ConnectorConfig::default()
    };
    let Err(error) = missing_compatibility.validate(0) else {
        anyhow::bail!("RPC endpoint without Solana RPC compatibility should fail");
    };
    assert!(error
        .to_string()
        .contains("solana_rpc_compatibility is required when rpc is configured"));

    let mut compatibility_without_endpoint = ConnectorConfig {
        name: "websocket".to_string(),
        provider: "solana-validator".to_string(),
        features: Vec::new(),
        solana_rpc_compatibility: Some(SolanaRpcCompatibility::Core),
        websocket: Some(crate::config::WebsocketTransportConfig::new(
            "wss://ws.example".to_string(),
        )),
        ..ConnectorConfig::default()
    };
    let Err(error) = compatibility_without_endpoint.validate(0) else {
        anyhow::bail!("Solana RPC compatibility without RPC endpoint should fail");
    };
    assert!(error
        .to_string()
        .contains("solana_rpc_compatibility requires a configured rpc transport"));

    let mut history_on_core = ConnectorConfig {
        name: "core".to_string(),
        provider: "solana-validator".to_string(),
        features: vec![ProviderFeature::HistoricalData],
        solana_rpc_compatibility: Some(SolanaRpcCompatibility::Core),
        rpc: Some(crate::config::RpcTransportConfig::new(
            "https://rpc.example".to_string(),
        )),
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
fn rejects_invalid_connector_transport_urls() -> Result<()> {
    let cases = [
        (
            ConnectorConfig {
                name: "rpc".to_string(),
                provider: "solana-validator".to_string(),
                features: Vec::new(),
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Core),
                rpc: Some(crate::config::RpcTransportConfig::new(
                    "ftp://rpc.example".to_string(),
                )),
                ..ConnectorConfig::default()
            },
            "connectors[0].rpc.url must use http:// or https:// with a host",
        ),
        (
            ConnectorConfig {
                name: "rpc".to_string(),
                provider: "solana-validator".to_string(),
                features: Vec::new(),
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Core),
                rpc: Some(crate::config::RpcTransportConfig::new(
                    "http:///rpc".to_string(),
                )),
                ..ConnectorConfig::default()
            },
            "connectors[0].rpc.url must use http:// or https:// with a host",
        ),
        (
            ConnectorConfig {
                name: "rpc".to_string(),
                provider: "solana-validator".to_string(),
                features: Vec::new(),
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Core),
                rpc: Some(crate::config::RpcTransportConfig::new("   ".to_string())),
                ..ConnectorConfig::default()
            },
            "connectors[0].rpc.url must not be empty",
        ),
        (
            ConnectorConfig {
                name: "websocket".to_string(),
                provider: "solana-validator".to_string(),
                features: Vec::new(),
                websocket: Some(crate::config::WebsocketTransportConfig::new(
                    "ws:///socket".to_string(),
                )),
                ..ConnectorConfig::default()
            },
            "connectors[0].websocket.url must use ws:// or wss:// with a host",
        ),
        (
            ConnectorConfig {
                name: "websocket".to_string(),
                provider: "solana-validator".to_string(),
                features: Vec::new(),
                websocket: Some(crate::config::WebsocketTransportConfig::new(
                    "   ".to_string(),
                )),
                ..ConnectorConfig::default()
            },
            "connectors[0].websocket.url must not be empty",
        ),
        (
            ConnectorConfig {
                name: "provider-api".to_string(),
                provider: "jito".to_string(),
                features: vec![ProviderFeature::JitoMevRewards],
                provider_api: Some(crate::config::HttpTransportConfig::new("   ".to_string())),
                ..ConnectorConfig::default()
            },
            "connectors[0].provider_api.url must not be empty",
        ),
    ];

    for (mut connector, expected) in cases {
        let Err(error) = connector.validate(0) else {
            anyhow::bail!("invalid connector transport URL should fail");
        };
        assert!(error.to_string().contains(expected), "{error:#}");
    }
    Ok(())
}

#[test]
fn provider_features_deserialize_and_export_stable_labels() -> Result<()> {
    let connector: ConnectorConfig = toml::from_str(
        r#"
name = "test"
provider = "jito"
features = ["historical-data", "jito-mev-rewards"]
solana_rpc_compatibility = "extended"
rpc = { url = "https://rpc.example" }
websocket = { url = "wss://rpc.example" }
provider_api = { url = "https://kobe.mainnet.jito.network/api/v1" }
"#,
    )?;

    assert_eq!(connector.provider, "jito");
    assert_eq!(
        connector.features,
        [
            ProviderFeature::HistoricalData,
            ProviderFeature::JitoMevRewards
        ]
    );
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
        ["historical-data", "jito-mev-rewards"]
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
rpc = {{ url = "https://rpc.example" }}
"#
            ),
        )?;
        let message = format!("{error:#}");
        assert!(
            message.contains(&format!("unknown field `{field_name}`")),
            "{message}"
        );
    }

    for (field_name, setting) in [
        ("rpc_url", r#"rpc_url = "https://rpc.example""#),
        ("ws_url", r#"ws_url = "wss://rpc.example""#),
        ("rpc_timeout_sec", "rpc_timeout_sec = 10"),
        ("ws_timeout_sec", "ws_timeout_sec = 10"),
        (
            "provider_api_url",
            r#"provider_api_url = "https://provider.example/api/v1""#,
        ),
        ("provider_api_timeout_sec", "provider_api_timeout_sec = 10"),
        ("commitment", r#"commitment = "confirmed""#),
        ("transport_mode", r#"transport_mode = "poll-http""#),
    ] {
        let connector_error = config_load_error(
            field_name,
            &format!(
                r#"
network = "testnet"

[[connectors]]
name = "primary"
provider = "solana"
features = []
rpc = {{ url = "https://rpc.example" }}
{setting}
"#
            ),
        )?;
        let connector_message = format!("{connector_error:#}");
        assert!(
            connector_message.contains(&format!("unknown field `{field_name}`")),
            "{connector_message}"
        );
    }

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
            websocket: Some(crate::config::WebsocketTransportConfig::new(
                "wss://ws.helius.example".to_string(),
            )),
            ..ConnectorConfig::default()
        }],
        ..Config::default()
    };

    match config.validate() {
        Ok(()) => anyhow::bail!("a websocket-only list should be rejected"),
        Err(error) => assert!(error
            .to_string()
            .contains("at least one connector with an rpc transport is required")),
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
    connector.websocket = Some(crate::config::WebsocketTransportConfig::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let mut config = Config {
        connectors: vec![connector],
        ..Config::default()
    };

    match config.validate() {
        Ok(()) => anyhow::bail!("invalid websocket URL should be rejected"),
        Err(error) => assert!(error
            .to_string()
            .contains("connectors[0].websocket.url must use ws:// or wss:// with a host")),
    }
    Ok(())
}

#[test]
fn rejects_blank_transport_url() -> Result<()> {
    let mut connector = rpc_connector("primary");
    connector.websocket = Some(crate::config::WebsocketTransportConfig::new(
        "   ".to_string(),
    ));
    let Err(error) = connector.validate(0) else {
        anyhow::bail!("a declared transport with a blank URL should fail");
    };

    assert!(error
        .to_string()
        .contains("connectors[0].websocket.url must not be empty"));
    Ok(())
}

#[test]
fn rejects_websocket_route_without_websocket_transport() -> Result<()> {
    let mut config = Config {
        connectors: vec![rpc_connector("primary")],
        modes: modes_for("primary", Some("primary"), false),
        ..Config::default()
    };
    let Err(error) = config.validate() else {
        anyhow::bail!("a WebSocket route requires a WebSocket transport");
    };

    assert!(error.to_string().contains(
        "modes.health.commands.ws_health references connector \"primary\", which has no configured WebSocket endpoint"
    ));
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
                websocket: Some(crate::config::WebsocketTransportConfig::new(
                    "wss://ws.example".to_string(),
                )),
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
                    recent_block_fee_rewards: vec![],
                    jito_mev_rewards: vec![],
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

#[test]
fn validates_block_fee_reward_feature_commitment_and_route() -> Result<()> {
    let mut block_rewards_on_core = rpc_connector("block-rewards");
    block_rewards_on_core.features = vec![ProviderFeature::BlockFeeRewards];
    block_rewards_on_core.solana_rpc_compatibility = Some(SolanaRpcCompatibility::Core);
    let Err(error) = block_rewards_on_core.validate(0) else {
        anyhow::bail!("block fee rewards on core Solana RPC should fail");
    };
    assert!(error.to_string().contains(
        "features entry \"block-fee-rewards\" requires solana_rpc_compatibility = \"extended\""
    ));

    let mut processed = rpc_connector("block-rewards");
    processed.features.push(ProviderFeature::BlockFeeRewards);
    processed
        .rpc
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("missing test RPC transport"))?
        .commitment = "processed".to_string();
    let Err(error) = processed.validate(0) else {
        anyhow::bail!("processed commitment for block fee rewards should fail");
    };
    assert!(error.to_string().contains(
        "features entry \"block-fee-rewards\" requires rpc.commitment = \"confirmed\" or \"finalized\""
    ));

    let mut config = Config {
        connectors: vec![rpc_connector("primary")],
        modes: modes_for("primary", None, true),
        ..Config::default()
    };
    config.modes.economics.validator = Some(WatchedValidatorConfig {
        identity: Some("id-1".to_string()),
        vote_account: Some("vote-1".to_string()),
        labels: vec![],
    });
    config.modes.economics.commands.recent_block_fee_rewards = vec!["primary".to_string()];
    let Err(error) = config.validate() else {
        anyhow::bail!("block fee reward route without declared feature should fail");
    };
    assert!(error
        .to_string()
        .contains("requires provider feature \"block-fee-rewards\""));

    config.connectors[0]
        .features
        .push(ProviderFeature::BlockFeeRewards);
    config.validate()
}

#[test]
fn validates_jito_provider_api_feature_and_route() -> Result<()> {
    let mut missing_endpoint = ConnectorConfig {
        name: "jito".to_string(),
        provider: "jito".to_string(),
        features: vec![ProviderFeature::JitoMevRewards],
        websocket: Some(crate::config::WebsocketTransportConfig::new(
            "wss://example.invalid".to_string(),
        )),
        ..ConnectorConfig::default()
    };
    let Err(error) = missing_endpoint.validate(0) else {
        anyhow::bail!("Jito feature without provider API endpoint should fail");
    };
    assert!(error
        .to_string()
        .contains("features entry \"jito-mev-rewards\" requires provider_api"));

    let mut invalid_url = ConnectorConfig {
        name: "jito".to_string(),
        provider: "jito".to_string(),
        features: vec![ProviderFeature::JitoMevRewards],
        provider_api: Some(crate::config::HttpTransportConfig::new(
            "ftp://jito.example/api/v1".to_string(),
        )),
        ..ConnectorConfig::default()
    };
    let Err(error) = invalid_url.validate(0) else {
        anyhow::bail!("non-HTTP provider API URL should fail");
    };
    assert!(error
        .to_string()
        .contains("connectors[0].provider_api.url must use http:// or https:// with a host"));

    let mut jito = ConnectorConfig {
        name: "jito".to_string(),
        provider: "jito".to_string(),
        features: vec![ProviderFeature::JitoMevRewards],
        provider_api: Some(crate::config::HttpTransportConfig::new(
            "https://kobe.mainnet.jito.network/api/v1".to_string(),
        )),
        ..ConnectorConfig::default()
    };
    jito.validate(1)?;

    let mut config = Config {
        connectors: vec![rpc_connector("primary"), jito],
        modes: modes_for("primary", None, true),
        ..Config::default()
    };
    config.modes.economics.validator = Some(WatchedValidatorConfig {
        identity: Some("id-1".to_string()),
        vote_account: Some("vote-1".to_string()),
        labels: vec![],
    });
    config.modes.economics.commands.jito_mev_rewards = vec!["jito".to_string()];
    config.validate()?;

    config.connectors[1].features.clear();
    let Err(error) = config.validate() else {
        anyhow::bail!("Jito reward route without declared feature should fail");
    };
    assert!(error
        .to_string()
        .contains("requires provider feature \"jito-mev-rewards\""));
    Ok(())
}

#[test]
fn normalizes_transport_headers_and_redacts_credentials() -> Result<()> {
    let mut connector: ConnectorConfig = toml::from_str(
        r#"
name = "primary"
provider = "solana"
features = []
solana_rpc_compatibility = "core"
rpc = { url = "https://rpc.example/?api-key=url-secret", headers = { "X-API-Key" = "header-secret" } }
"#,
    )?;

    connector.validate(0)?;

    let rpc = connector
        .rpc
        .as_ref()
        .context("validated RPC transport is missing")?;
    assert!(rpc.headers.values.contains_key("x-api-key"));
    let debug = format!("{connector:?}");
    assert!(debug.contains("x-api-key"));
    assert!(!debug.contains("header-secret"));
    assert!(!debug.contains("url-secret"));
    assert!(!debug.contains("rpc.example"));
    Ok(())
}

#[test]
fn toml_parse_errors_do_not_expose_expanded_header_credentials() -> Result<()> {
    let test_dir = unique_test_dir("header-parse-redaction");
    fs::create_dir_all(&test_dir)
        .with_context(|| format!("failed to create {}", test_dir.display()))?;
    let config_path = test_dir.join("watcher.toml");
    fs::write(
        &config_path,
        r#"
network = "testnet"

[[connectors]]
name = "primary"
provider = "solana"
features = []
solana_rpc_compatibility = "core"
rpc = { url = "https://rpc.example", headers = { authorization = "Bearer ${AUTH_SECRET}" }
"#,
    )
    .with_context(|| format!("failed to write {}", config_path.display()))?;
    let secret = "expanded-header-secret";

    let error = match Config::load_with(&config_path, |raw| {
        expand_env_vars_with(raw, |key| {
            Ok((key == "AUTH_SECRET").then(|| secret.to_string()))
        })
    }) {
        Ok(_) => anyhow::bail!("malformed TOML should be rejected"),
        Err(error) => error,
    };
    fs::remove_dir_all(&test_dir)
        .with_context(|| format!("failed to remove {}", test_dir.display()))?;

    let message = format!("{error:#}");
    assert!(message.contains("failed to parse TOML config"), "{message}");
    assert!(message.contains("at line 9, column"), "{message}");
    assert!(
        !message.contains(secret),
        "TOML parse errors must not expose expanded header credentials"
    );
    Ok(())
}

#[test]
fn rejects_unsafe_transport_headers_timeouts_and_tls_overrides() -> Result<()> {
    let cases = [
        (
            "rpc-reserved-header",
            r#"rpc = { url = "https://rpc.example", headers = { Host = "ignored" } }"#,
            "connectors[0].rpc.headers must not override protocol-owned header \"host\"",
        ),
        (
            "websocket-reserved-header",
            r#"rpc = { url = "https://rpc.example" }
websocket = { url = "wss://rpc.example", headers = { Connection = "ignored" } }"#,
            "connectors[0].websocket.headers must not override protocol-owned header \"connection\"",
        ),
        (
            "invalid-header-value",
            r#"rpc = { url = "https://rpc.example", headers = { authorization = "Bearer header-value-secret\nInjected: yes" } }"#,
            "connectors[0].rpc.headers.authorization contains an invalid header value",
        ),
        (
            "duplicate-header-name",
            r#"rpc = { url = "https://rpc.example", headers = { Authorization = "first", authorization = "second" } }"#,
            "connectors[0].rpc.headers contains duplicate header name \"authorization\"",
        ),
        (
            "invalid-http-timeouts",
            r#"rpc = { url = "https://rpc.example", request_timeout_sec = 5, connect_timeout_sec = 6 }"#,
            "connectors[0].rpc.connect_timeout_sec must not exceed request_timeout_sec",
        ),
        (
            "invalid-websocket-timeout",
            r#"rpc = { url = "https://rpc.example" }
websocket = { url = "wss://rpc.example", handshake_timeout_sec = 0 }"#,
            "connectors[0].websocket.handshake_timeout_sec must be greater than 0",
        ),
        (
            "insecure-rpc-tls-override",
            r#"rpc = { url = "https://rpc.example", danger_accept_invalid_certs = true }"#,
            "unknown field `danger_accept_invalid_certs`",
        ),
        (
            "insecure-websocket-tls-override",
            r#"rpc = { url = "https://rpc.example" }
websocket = { url = "wss://rpc.example", danger_accept_invalid_certs = true }"#,
            "unknown field `danger_accept_invalid_certs`",
        ),
    ];

    for (name, transport, expected) in cases {
        let raw = format!(
            r#"
network = "testnet"

[[connectors]]
name = "primary"
provider = "solana"
features = []
solana_rpc_compatibility = "core"
{transport}
"#
        );
        let error = config_load_error(name, &raw)?;
        let message = format!("{error:#}");
        assert!(message.contains(expected), "{name}: {message}");
        assert!(
            !message.contains("header-value-secret"),
            "{name}: transport errors must not expose header values"
        );
    }
    Ok(())
}
