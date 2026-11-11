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
                rpc: Some(crate::RpcTransportConfig::new(
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
                rpc: Some(crate::RpcTransportConfig::new(
                    "https://rpc.example".to_string(),
                )),
                ..ConnectorConfig::default()
            },
            "contains duplicate provider feature \"historical-data\"",
        ),
    ];

    for (connector, expected) in cases {
        let config = Config {
            connectors: vec![connector],
            ..Config::default()
        };
        let Err(error) = config.into_validated() else {
            anyhow::bail!("invalid provider feature declaration should be rejected");
        };
        assert!(error.to_string().contains(expected), "{error:#}");
    }
    Ok(())
}

#[test]
fn validates_solana_rpc_compatibility_against_endpoint_and_features() -> Result<()> {
    let missing_compatibility = ConnectorConfig {
        name: "rpc".to_string(),
        provider: "solana-validator".to_string(),
        features: Vec::new(),
        rpc: Some(crate::RpcTransportConfig::new(
            "https://rpc.example".to_string(),
        )),
        ..ConnectorConfig::default()
    };
    let Err(error) = missing_compatibility.into_validated(0) else {
        anyhow::bail!("RPC endpoint without Solana RPC compatibility should fail");
    };
    assert!(error
        .to_string()
        .contains("solana_rpc_compatibility is required when rpc is configured"));

    let compatibility_without_endpoint = ConnectorConfig {
        name: "websocket".to_string(),
        provider: "solana-validator".to_string(),
        features: Vec::new(),
        solana_rpc_compatibility: Some(SolanaRpcCompatibility::Core),
        websocket: Some(crate::WebsocketTransportConfig::new(
            "wss://ws.example".to_string(),
        )),
        ..ConnectorConfig::default()
    };
    let Err(error) = compatibility_without_endpoint.into_validated(0) else {
        anyhow::bail!("Solana RPC compatibility without RPC endpoint should fail");
    };
    assert!(error
        .to_string()
        .contains("solana_rpc_compatibility requires a configured rpc transport"));

    let history_on_core = ConnectorConfig {
        name: "core".to_string(),
        provider: "solana-validator".to_string(),
        features: vec![ProviderFeature::HistoricalData],
        solana_rpc_compatibility: Some(SolanaRpcCompatibility::Core),
        rpc: Some(crate::RpcTransportConfig::new(
            "https://rpc.example".to_string(),
        )),
        ..ConnectorConfig::default()
    };
    let Err(error) = history_on_core.into_validated(0) else {
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
                rpc: Some(crate::RpcTransportConfig::new(
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
                rpc: Some(crate::RpcTransportConfig::new("http:///rpc".to_string())),
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
                rpc: Some(crate::RpcTransportConfig::new("   ".to_string())),
                ..ConnectorConfig::default()
            },
            "connectors[0].rpc.url must not be empty",
        ),
        (
            ConnectorConfig {
                name: "websocket".to_string(),
                provider: "solana-validator".to_string(),
                features: Vec::new(),
                websocket: Some(crate::WebsocketTransportConfig::new(
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
                websocket: Some(crate::WebsocketTransportConfig::new("   ".to_string())),
                ..ConnectorConfig::default()
            },
            "connectors[0].websocket.url must not be empty",
        ),
        (
            ConnectorConfig {
                name: "provider-api".to_string(),
                provider: "jito".to_string(),
                features: vec![ProviderFeature::JitoMevRewards],
                provider_api: Some(crate::HttpTransportConfig::new("   ".to_string())),
                ..ConnectorConfig::default()
            },
            "connectors[0].provider_api.url must not be empty",
        ),
    ];

    for (connector, expected) in cases {
        let Err(error) = connector.into_validated(0) else {
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
    let config = Config::default();

    match config.into_validated() {
        Ok(_) => anyhow::bail!("missing connectors should be rejected"),
        Err(error) => assert!(error
            .to_string()
            .contains("at least one connector is required")),
    }
    Ok(())
}

#[test]
fn rejects_ws_only_connector_list() -> Result<()> {
    let config = Config {
        connectors: vec![ConnectorConfig {
            name: "ws-only".to_string(),
            provider: "helius".to_string(),
            features: Vec::new(),
            websocket: Some(crate::WebsocketTransportConfig::new(
                "wss://ws.helius.example".to_string(),
            )),
            ..ConnectorConfig::default()
        }],
        ..Config::default()
    };

    match config.into_validated() {
        Ok(_) => anyhow::bail!("a websocket-only list should be rejected"),
        Err(error) => assert!(error
            .to_string()
            .contains("at least one connector with an rpc transport is required")),
    }
    Ok(())
}

#[test]
fn rejects_duplicate_connector_names() -> Result<()> {
    let config = Config {
        connectors: vec![rpc_connector("primary"), rpc_connector("primary")],
        ..Config::default()
    };

    match config.into_validated() {
        Ok(_) => anyhow::bail!("duplicate connector names should be rejected"),
        Err(error) => assert!(error
            .to_string()
            .contains("duplicate connector name \"primary\"")),
    }
    Ok(())
}

#[test]
fn rejects_invalid_ws_scheme() -> Result<()> {
    let mut connector = rpc_connector("primary");
    connector.websocket = Some(crate::WebsocketTransportConfig::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let config = Config {
        connectors: vec![connector],
        ..Config::default()
    };

    match config.into_validated() {
        Ok(_) => anyhow::bail!("invalid websocket URL should be rejected"),
        Err(error) => assert!(error
            .to_string()
            .contains("connectors[0].websocket.url must use ws:// or wss:// with a host")),
    }
    Ok(())
}

#[test]
fn rejects_blank_transport_url() -> Result<()> {
    let mut connector = rpc_connector("primary");
    connector.websocket = Some(crate::WebsocketTransportConfig::new("   ".to_string()));
    let Err(error) = connector.into_validated(0) else {
        anyhow::bail!("a declared transport with a blank URL should fail");
    };

    assert!(error
        .to_string()
        .contains("connectors[0].websocket.url must not be empty"));
    Ok(())
}

#[test]
fn rejects_websocket_route_without_websocket_transport() -> Result<()> {
    let config = Config {
        connectors: vec![rpc_connector("primary")],
        modes: modes_for("primary", Some("primary"), false),
        ..Config::default()
    };
    let Err(error) = config.into_validated() else {
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
    let Err(error) = unknown.into_validated() else {
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
    let Err(error) = duplicate.into_validated() else {
        anyhow::bail!("duplicate route connector should be rejected");
    };
    assert!(error
        .to_string()
        .contains("contains duplicate connector name \"primary\""));
    Ok(())
}

#[test]
fn expands_backups_only_for_their_exact_source_requirement() -> Result<()> {
    let mut primary = rpc_connector("primary");
    primary.backups = std::collections::BTreeMap::from([
        (
            ModeSourceRequirement::RpcCore,
            vec!["core-backup".to_string(), "final-core-backup".to_string()],
        ),
        (
            ModeSourceRequirement::RpcExtended,
            vec!["extended-backup".to_string()],
        ),
    ]);
    let mut core_backup = rpc_connector("core-backup");
    core_backup.features.clear();
    core_backup.solana_rpc_compatibility = Some(SolanaRpcCompatibility::Core);
    let mut final_core_backup = rpc_connector("final-core-backup");
    final_core_backup.features.clear();
    final_core_backup.solana_rpc_compatibility = Some(SolanaRpcCompatibility::Core);
    let config = Config {
        connectors: vec![
            primary,
            core_backup,
            final_core_backup,
            rpc_connector("extended-backup"),
        ],
        modes: modes_for("primary", None, false),
        ..Config::default()
    }
    .into_validated()?;

    assert_eq!(
        config.modes.consensus.commands.epoch_info,
        ["primary", "core-backup", "final-core-backup"]
    );
    assert_eq!(
        config.modes.consensus.commands.recent_performance_samples,
        ["primary", "extended-backup"]
    );
    assert_eq!(
        config.modes.leadership.commands.current_block_production,
        ["primary", "extended-backup"]
    );
    assert!(config.modes.health.commands.ws_health.is_empty());
    Ok(())
}

#[test]
fn rejects_empty_self_referential_and_duplicate_backup_lists() -> Result<()> {
    let cases = [
        (
            Vec::new(),
            "backups.rpc-core must contain at least one connector name",
        ),
        (
            vec!["   ".to_string()],
            "backups.rpc-core[0] must not be empty",
        ),
        (
            vec!["primary".to_string()],
            "backups.rpc-core must not reference connector \"primary\" itself",
        ),
        (
            vec!["backup".to_string(), "backup".to_string()],
            "backups.rpc-core contains duplicate connector name \"backup\"",
        ),
    ];

    for (backups, expected) in cases {
        let mut connector = rpc_connector("primary");
        connector
            .backups
            .insert(ModeSourceRequirement::RpcCore, backups);
        let Err(error) = connector.into_validated(0) else {
            anyhow::bail!("invalid backup list should be rejected");
        };
        assert!(error.to_string().contains(expected), "{error:#}");
    }
    Ok(())
}

#[test]
fn rejects_incompatible_toml_backup_before_runtime_construction() -> Result<()> {
    let error = config_load_error(
        "incompatible-typed-backup",
        r#"
network = "testnet"

[[connectors]]
name = "primary"
provider = "solana-validator"
features = []
solana_rpc_compatibility = "extended"
rpc = { url = "https://primary.example" }

[connectors.backups]
rpc-extended = ["core-backup"]

[[connectors]]
name = "core-backup"
provider = "solana-validator"
features = []
solana_rpc_compatibility = "core"
rpc = { url = "https://backup.example" }

[modes.health.commands]
rpc_health = ["primary"]

[modes.consensus.commands]
epoch_info = ["primary"]
vote_accounts = ["primary"]
"#,
    )?;

    assert!(
        error
            .to_string()
            .contains("connectors[0].backups.rpc-extended[0] references connector \"core-backup\", which does not satisfy source requirement \"rpc-extended\""),
        "{error:#}"
    );
    Ok(())
}

#[test]
fn rejects_unknown_incompatible_and_duplicate_expanded_backups() -> Result<()> {
    let mut unknown = rpc_connector("primary");
    unknown
        .backups
        .insert(ModeSourceRequirement::RpcCore, vec!["missing".to_string()]);
    let Err(error) = Config {
        connectors: vec![unknown],
        modes: modes_for("primary", None, false),
        ..Config::default()
    }
    .into_validated() else {
        anyhow::bail!("unknown backup connector should be rejected");
    };
    assert!(
        error
            .to_string()
            .contains("references unknown connector \"missing\""),
        "{error:#}"
    );

    let mut incompatible = rpc_connector("primary");
    incompatible.backups.insert(
        ModeSourceRequirement::RpcExtended,
        vec!["core-backup".to_string()],
    );
    let mut core_backup = rpc_connector("core-backup");
    core_backup.features.clear();
    core_backup.solana_rpc_compatibility = Some(SolanaRpcCompatibility::Core);
    let Err(error) = Config {
        connectors: vec![incompatible, core_backup],
        modes: modes_for("primary", None, false),
        ..Config::default()
    }
    .into_validated() else {
        anyhow::bail!("incompatible backup connector should be rejected");
    };
    assert!(
        error
            .to_string()
            .contains("does not satisfy source requirement \"rpc-extended\""),
        "{error:#}"
    );

    let mut duplicate = rpc_connector("primary");
    duplicate
        .backups
        .insert(ModeSourceRequirement::RpcCore, vec!["backup".to_string()]);
    let mut modes = modes_for("primary", None, false);
    modes
        .consensus
        .commands
        .epoch_info
        .push("backup".to_string());
    let Err(error) = Config {
        connectors: vec![duplicate, rpc_connector("backup")],
        modes,
        ..Config::default()
    }
    .into_validated() else {
        anyhow::bail!("expanded duplicate connector should be rejected");
    };
    assert!(
        error
            .to_string()
            .contains("contains duplicate connector name \"backup\""),
        "{error:#}"
    );
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
                websocket: Some(crate::WebsocketTransportConfig::new(
                    "wss://ws.example".to_string(),
                )),
                ..ConnectorConfig::default()
            },
        ],
        modes: modes_for("primary", Some("ws-only"), false),
        ..Config::default()
    };
    config.modes.leadership.commands.current_block_production = vec!["ws-only".to_string()];

    let Err(error) = config.into_validated() else {
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
    let config = Config {
        connectors: vec![connector],
        modes: modes_for("core", None, false),
        ..Config::default()
    };

    let Err(error) = config.into_validated() else {
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
    let config = Config {
        connectors: vec![connector],
        modes,
        ..Config::default()
    };

    config.into_validated()?;
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

    let Err(error) = config.into_validated() else {
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

    config.into_validated()?;
    Ok(())
}

#[test]
fn rejects_economics_routes_while_mode_is_disabled() -> Result<()> {
    let mut config = Config {
        connectors: vec![rpc_connector("primary")],
        modes: modes_for("primary", None, false),
        ..Config::default()
    };
    config.modes.economics.commands.inflation_rewards = vec!["primary".to_string()];

    let Err(error) = config.into_validated() else {
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

    let config = config.into_validated()?;

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
    let config = Config {
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

    match config.into_validated() {
        Ok(_) => anyhow::bail!("economics validator should be rejected"),
        Err(error) => assert!(error
            .to_string()
            .contains("modes.economics.validator requires modes.economics.enabled = true")),
    }
    Ok(())
}

#[test]
fn rejects_missing_economics_validator_in_single_validator_mode() -> Result<()> {
    let config = Config {
        connectors: vec![rpc_connector("primary")],
        modes: ModesConfig {
            economics: EconomicsModeConfig {
                enabled: true,
                validator: None,
                reserve_wallets: vec![],
                commands: EconomicsCommands {
                    account_balances: vec!["primary".to_string()],
                    reserve_wallet_balances: vec![],
                    inflation_rewards: vec!["primary".to_string()],
                    reward_epoch_block_production: vec!["primary".to_string()],
                    recent_block_fee_rewards: vec![],
                    jito_mev_rewards: vec![],
                    trillium_mev_capture: vec![],
                },
            },
            ..modes_for("primary", None, false)
        },
        ..Config::default()
    };

    match config.into_validated() {
        Ok(_) => anyhow::bail!("missing economics validator should be rejected"),
        Err(error) => assert!(error
            .to_string()
            .contains("modes.economics.enabled requires modes.economics.validator")),
    }
    Ok(())
}
#[test]
fn validates_reserve_wallets_and_their_required_command_route() -> Result<()> {
    let target = || WatchedValidatorConfig {
        identity: Some("id-1".to_string()),
        vote_account: Some("vote-1".to_string()),
        labels: vec![],
    };

    let mut disabled = Config {
        connectors: vec![rpc_connector("primary")],
        modes: modes_for("primary", None, false),
        ..Config::default()
    };
    disabled.modes.economics.reserve_wallets = vec!["reserve-1".to_string()];
    let Err(error) = disabled.into_validated() else {
        anyhow::bail!("reserve wallets should require economics mode");
    };
    assert!(error
        .to_string()
        .contains("modes.economics.reserve_wallets requires modes.economics.enabled = true"));

    let mut missing_route = Config {
        connectors: vec![rpc_connector("primary")],
        modes: modes_for("primary", None, true),
        ..Config::default()
    };
    missing_route.modes.economics.validator = Some(target());
    missing_route.modes.economics.reserve_wallets = vec![" reserve-1 ".to_string()];
    let Err(error) = missing_route.into_validated() else {
        anyhow::bail!("configured reserve wallets should require a command route");
    };
    assert!(error.to_string().contains(
        "modes.economics.commands.reserve_wallet_balances must contain at least one connector name"
    ));

    let mut route_without_wallet = Config {
        connectors: vec![rpc_connector("primary")],
        modes: modes_for("primary", None, true),
        ..Config::default()
    };
    route_without_wallet.modes.economics.validator = Some(target());
    route_without_wallet
        .modes
        .economics
        .commands
        .reserve_wallet_balances = vec!["primary".to_string()];
    let Err(error) = route_without_wallet.into_validated() else {
        anyhow::bail!("reserve-wallet command route should require wallets");
    };
    assert!(error.to_string().contains(
        "modes.economics.commands.reserve_wallet_balances requires modes.economics.reserve_wallets"
    ));

    let mut duplicates = Config {
        connectors: vec![rpc_connector("primary")],
        modes: modes_for("primary", None, true),
        ..Config::default()
    };
    duplicates.modes.economics.validator = Some(target());
    duplicates.modes.economics.reserve_wallets =
        vec![" reserve-1 ".to_string(), "reserve-1".to_string()];
    duplicates.modes.economics.commands.reserve_wallet_balances = vec!["primary".to_string()];
    let Err(error) = duplicates.into_validated() else {
        anyhow::bail!("duplicate reserve wallets should be rejected");
    };
    assert!(error
        .to_string()
        .contains("modes.economics.reserve_wallets contains duplicate wallet \"reserve-1\""));

    let mut valid = Config {
        connectors: vec![rpc_connector("primary")],
        modes: modes_for("primary", None, true),
        ..Config::default()
    };
    valid.modes.economics.validator = Some(target());
    valid.modes.economics.reserve_wallets =
        vec![" reserve-1 ".to_string(), "reserve-2".to_string()];
    valid.modes.economics.commands.reserve_wallet_balances = vec!["primary".to_string()];
    let valid = valid.into_validated()?;
    assert_eq!(
        valid.modes.economics.reserve_wallets,
        ["reserve-1", "reserve-2"]
    );
    assert_eq!(
        valid.modes.route(CommandId::ReserveWalletBalances),
        ["primary"]
    );
    Ok(())
}

#[test]
fn validates_block_fee_reward_feature_commitment_and_route() -> Result<()> {
    let mut block_rewards_on_core = rpc_connector("block-rewards");
    block_rewards_on_core.features = vec![ProviderFeature::BlockFeeRewards];
    block_rewards_on_core.solana_rpc_compatibility = Some(SolanaRpcCompatibility::Core);
    let Err(error) = block_rewards_on_core.into_validated(0) else {
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
    let Err(error) = processed.into_validated(0) else {
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
    let Err(error) = config.clone().into_validated() else {
        anyhow::bail!("block fee reward route without declared feature should fail");
    };
    assert!(error
        .to_string()
        .contains("requires provider feature \"block-fee-rewards\""));

    config.connectors[0]
        .features
        .push(ProviderFeature::BlockFeeRewards);
    config.into_validated()?;
    Ok(())
}

#[test]
fn validates_jito_provider_api_feature_and_route() -> Result<()> {
    let missing_endpoint = ConnectorConfig {
        name: "jito".to_string(),
        provider: "jito".to_string(),
        features: vec![ProviderFeature::JitoMevRewards],
        websocket: Some(crate::WebsocketTransportConfig::new(
            "wss://example.invalid".to_string(),
        )),
        ..ConnectorConfig::default()
    };
    let Err(error) = missing_endpoint.into_validated(0) else {
        anyhow::bail!("Jito feature without provider API endpoint should fail");
    };
    assert!(error
        .to_string()
        .contains("features entry \"jito-mev-rewards\" requires provider_api"));

    let invalid_url = ConnectorConfig {
        name: "jito".to_string(),
        provider: "jito".to_string(),
        features: vec![ProviderFeature::JitoMevRewards],
        provider_api: Some(crate::HttpTransportConfig::new(
            "ftp://jito.example/api/v1".to_string(),
        )),
        ..ConnectorConfig::default()
    };
    let Err(error) = invalid_url.into_validated(0) else {
        anyhow::bail!("non-HTTP provider API URL should fail");
    };
    assert!(error
        .to_string()
        .contains("connectors[0].provider_api.url must use http:// or https:// with a host"));

    let jito = ConnectorConfig {
        name: "jito".to_string(),
        provider: "jito".to_string(),
        features: vec![ProviderFeature::JitoMevRewards],
        provider_api: Some(crate::HttpTransportConfig::new(
            "https://kobe.mainnet.jito.network/api/v1".to_string(),
        )),
        ..ConnectorConfig::default()
    };
    let jito = jito.into_validated(1)?;

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
    let mut config = config.into_validated()?;

    config.connectors[1].features.clear();
    let Err(error) = config.into_validated() else {
        anyhow::bail!("Jito reward route without declared feature should fail");
    };
    assert!(error
        .to_string()
        .contains("requires provider feature \"jito-mev-rewards\""));
    Ok(())
}

#[test]
fn validates_trillium_provider_api_feature_and_route() -> Result<()> {
    let missing_endpoint = ConnectorConfig {
        name: "trillium".to_string(),
        provider: "trillium".to_string(),
        features: vec![ProviderFeature::TrilliumMevCapture],
        websocket: Some(crate::WebsocketTransportConfig::new(
            "wss://example.invalid".to_string(),
        )),
        ..ConnectorConfig::default()
    };
    let Err(error) = missing_endpoint.into_validated(0) else {
        anyhow::bail!("Trillium feature without provider API endpoint should fail");
    };
    assert!(error
        .to_string()
        .contains("features entry \"trillium-mev-capture\" requires provider_api"));

    let incompatible_provider_features = ConnectorConfig {
        name: "mixed-provider-api".to_string(),
        provider: "mixed".to_string(),
        features: vec![
            ProviderFeature::JitoMevRewards,
            ProviderFeature::TrilliumMevCapture,
        ],
        provider_api: Some(crate::HttpTransportConfig::new(
            "https://api.trillium.so".to_string(),
        )),
        ..ConnectorConfig::default()
    };
    assert!(incompatible_provider_features.into_validated(1).is_err());

    let trillium = ConnectorConfig {
        name: "trillium".to_string(),
        provider: "trillium".to_string(),
        features: vec![ProviderFeature::TrilliumMevCapture],
        provider_api: Some(crate::HttpTransportConfig::new(
            "https://api.trillium.so".to_string(),
        )),
        ..ConnectorConfig::default()
    }
    .into_validated(1)?;
    let mut config = Config {
        connectors: vec![rpc_connector("primary"), trillium],
        modes: modes_for("primary", None, true),
        ..Config::default()
    };
    config.modes.economics.validator = Some(WatchedValidatorConfig {
        identity: Some("id-1".to_string()),
        vote_account: Some("vote-1".to_string()),
        labels: vec![],
    });
    config.modes.economics.commands.trillium_mev_capture = vec!["trillium".to_string()];
    let mut config = config.into_validated()?;

    config.connectors[1].features.clear();
    let Err(error) = config.into_validated() else {
        anyhow::bail!("Trillium route without declared feature should fail");
    };
    assert!(error
        .to_string()
        .contains("requires provider feature \"trillium-mev-capture\""));
    Ok(())
}

#[test]
fn normalizes_transport_headers_and_redacts_credentials() -> Result<()> {
    let connector: ConnectorConfig = toml::from_str(
        r#"
name = "primary"
provider = "solana"
features = []
solana_rpc_compatibility = "core"
rpc = { url = "https://rpc.example/?api-key=url-secret", headers = { "X-API-Key" = "header-secret" } }
"#,
    )?;

    let connector = connector.into_validated(0)?;

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
            "zero-rpc-response-limit",
            r#"rpc = { url = "https://rpc.example", response_size_limit_bytes = 0 }"#,
            "connectors[0].rpc.response_size_limit_bytes must be between 1 and 134217728",
        ),
        (
            "excessive-rpc-response-limit",
            r#"rpc = { url = "https://rpc.example", response_size_limit_bytes = 134217729 }"#,
            "connectors[0].rpc.response_size_limit_bytes must be between 1 and 134217728",
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
