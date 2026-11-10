use super::{config_mtime, RuntimeState};
use crate::{
    mode::CommandId,
    runtime::test_support::{
        start_capability_probe_server, start_controlled_capability_probe_server,
        start_header_recording_capability_probe_server, unique_test_dir,
    },
};
use anyhow::{Context, Result};
use std::{fs, time::UNIX_EPOCH};

#[test]
fn config_mtime_reads_existing_file() -> Result<()> {
    let test_dir = unique_test_dir("mtime");
    fs::create_dir_all(&test_dir)
        .with_context(|| format!("failed to create {}", test_dir.display()))?;
    let path = test_dir.join("mtime.txt");
    fs::write(&path, "test").with_context(|| format!("failed to write {}", path.display()))?;

    let modified = config_mtime(&path)?;
    assert!(modified.duration_since(UNIX_EPOCH).is_ok());

    fs::remove_dir_all(&test_dir)
        .with_context(|| format!("failed to remove directory {}", test_dir.display()))?;

    Ok(())
}

#[tokio::test]
async fn reload_if_needed_rebuilds_command_routes_and_resets_economics_flag() -> Result<()> {
    let (rpc_url, server_task) = start_capability_probe_server(None).await?;
    let test_dir = unique_test_dir("reload-order");
    fs::create_dir_all(&test_dir)
        .with_context(|| format!("failed to create {}", test_dir.display()))?;
    let config_path = test_dir.join("watcher.toml");
    fs::write(
        &config_path,
        r#"
network = "mainnet-beta"

[[connectors]]
name = "primary"
provider = "solana"
features = []
solana_rpc_compatibility = "extended"
rpc = { url = "https://primary.example" }

[[connectors]]
name = "fallback"
provider = "helius"
features = []
websocket = { url = "wss://fallback.example" }

[modes.health.commands]
rpc_health = ["primary"]
ws_health = ["fallback"]

[modes.consensus.commands]
epoch_info = ["primary"]
vote_accounts = ["primary"]
recent_performance_samples = ["primary"]

[modes.leadership.commands]
current_block_production = ["primary"]
current_leader_schedule = ["primary"]
next_leader_schedule = ["primary"]

[modes.economics]
enabled = false
"#,
    )
    .with_context(|| format!("failed to write {}", config_path.display()))?;

    let mut runtime = RuntimeState::load(
        config_path.clone(),
        crate::watcher_state::RuntimeTelemetry::unobserved(),
    )?;
    runtime.economics_capabilities_verified = true;
    runtime.config_modified = UNIX_EPOCH;

    fs::write(
        &config_path,
        format!(
            r#"
network = "mainnet-beta"

[[connectors]]
name = "fallback"
provider = "helius"
features = []
solana_rpc_compatibility = "extended"
rpc = {{ url = "{rpc_url}" }}
websocket = {{ url = "wss://fallback.example" }}

[[connectors]]
name = "replacement"
provider = "solana"
features = []
solana_rpc_compatibility = "extended"
rpc = {{ url = "{rpc_url}" }}

[modes.health.commands]
rpc_health = ["replacement", "fallback"]
ws_health = ["fallback"]

[modes.consensus.commands]
epoch_info = ["replacement", "fallback"]
vote_accounts = ["replacement", "fallback"]
recent_performance_samples = ["replacement", "fallback"]

[modes.leadership.commands]
current_block_production = ["fallback", "replacement"]
current_leader_schedule = ["replacement", "fallback"]
next_leader_schedule = ["replacement", "fallback"]

[modes.economics]
enabled = false
"#
        ),
    )
    .with_context(|| format!("failed to write {}", config_path.display()))?;

    runtime.reload_if_needed().await?;

    assert_eq!(runtime.config.connectors[0].name, "fallback");
    assert_eq!(runtime.config.connectors[1].name, "replacement");
    let rpc_health_route = runtime
        .sources
        .routes()
        .filter(|route| route.command == CommandId::RpcHealth)
        .map(|route| route.connector.to_string())
        .collect::<Vec<_>>();
    let block_production_route = runtime
        .sources
        .routes()
        .filter(|route| route.command == CommandId::CurrentBlockProduction)
        .map(|route| route.connector.to_string())
        .collect::<Vec<_>>();
    assert_eq!(rpc_health_route, ["replacement", "fallback"]);
    assert_eq!(block_production_route, ["fallback", "replacement"]);
    assert_ne!(runtime.config_modified, UNIX_EPOCH);
    assert!(!runtime.economics_capabilities_verified);

    fs::remove_dir_all(&test_dir)
        .with_context(|| format!("failed to remove directory {}", test_dir.display()))?;
    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn source_changing_reload_resets_failover_after_candidate_validation() -> Result<()> {
    fn config_toml(
        primary_url: &str,
        backup_url: &str,
        configured_backup: &str,
        rpc_options: &str,
    ) -> String {
        format!(
            r#"
network = "mainnet-beta"

[[connectors]]
name = "primary"
provider = "primary-provider"
features = []
solana_rpc_compatibility = "extended"
rpc = {{ url = "{primary_url}"{rpc_options} }}

[connectors.backups]
rpc-extended = ["{configured_backup}"]

[[connectors]]
name = "backup"
provider = "backup-provider"
features = []
solana_rpc_compatibility = "extended"
rpc = {{ url = "{backup_url}" }}

[modes.health.commands]
rpc_health = ["primary"]

[modes.consensus.commands]
epoch_info = ["primary"]
vote_accounts = ["primary"]
recent_performance_samples = ["primary"]

[modes.leadership.commands]
current_block_production = ["primary"]
"#
        )
    }

    let (primary_url, primary_control, primary_task) =
        start_controlled_capability_probe_server(None).await?;
    let (backup_url, backup_task) = start_capability_probe_server(None).await?;
    let test_dir = unique_test_dir("reload-failover");
    fs::create_dir_all(&test_dir)
        .with_context(|| format!("failed to create {}", test_dir.display()))?;
    let config_path = test_dir.join("watcher.toml");
    fs::write(
        &config_path,
        config_toml(&primary_url, &backup_url, "backup", ""),
    )
    .with_context(|| format!("failed to write {}", config_path.display()))?;
    let mut runtime = RuntimeState::load(
        config_path.clone(),
        crate::watcher_state::RuntimeTelemetry::unobserved(),
    )?;

    primary_control.set_recent_performance_failure(true);
    let failed_command = runtime
        .sources
        .recent_performance()
        .await?
        .context("backup should satisfy the failed command")?;
    assert_eq!(failed_command.source.connector.as_ref(), "backup");
    primary_control.set_recent_performance_failure(false);

    fs::write(
        &config_path,
        config_toml(&primary_url, &backup_url, "missing", ""),
    )
    .with_context(|| format!("failed to write {}", config_path.display()))?;
    runtime.config_modified = UNIX_EPOCH;
    if runtime.reload_if_needed().await.is_ok() {
        anyhow::bail!("invalid source-chain candidate should be rejected");
    }

    let preserved_failover = runtime
        .sources
        .recent_performance()
        .await?
        .context("active command fallback should survive a rejected reload")?;
    assert_eq!(preserved_failover.source.connector.as_ref(), "backup");

    fs::write(
        &config_path,
        config_toml(
            &primary_url,
            &backup_url,
            "backup",
            ", request_timeout_sec = 31",
        ),
    )
    .with_context(|| format!("failed to write {}", config_path.display()))?;
    runtime.reload_if_needed().await?;

    let reset_failover = runtime
        .sources
        .recent_performance()
        .await?
        .context("primary should be retried after accepted source-chain reload")?;
    assert_eq!(reset_failover.source.connector.as_ref(), "primary");

    fs::remove_dir_all(&test_dir)
        .with_context(|| format!("failed to remove directory {}", test_dir.display()))?;
    primary_task.abort();
    backup_task.abort();
    Ok(())
}

#[tokio::test]
async fn reload_validation_does_not_mutate_active_failover_state() -> Result<()> {
    fn config_toml(network: &str, primary_url: &str, backup_url: &str) -> String {
        format!(
            r#"
network = "{network}"
delinquent_slot_distance = 128

[[connectors]]
name = "primary"
provider = "primary-provider"
features = ["historical-data"]
solana_rpc_compatibility = "extended"
rpc = {{ url = "{primary_url}" }}

[connectors.backups]
rpc-core = ["backup"]

[[connectors]]
name = "backup"
provider = "backup-provider"
features = ["historical-data"]
solana_rpc_compatibility = "extended"
rpc = {{ url = "{backup_url}" }}

[modes.health.commands]
rpc_health = ["primary"]

[modes.consensus.commands]
epoch_info = ["primary"]
vote_accounts = ["primary"]

[modes.economics]
enabled = true

[modes.economics.validator]
identity = "id-1"
vote_account = "vote-1"
labels = []

[modes.economics.commands]
account_balances = ["primary"]
inflation_rewards = ["primary"]
reward_epoch_block_production = ["primary"]
"#
        )
    }

    let (primary_url, primary_control, primary_task) =
        start_controlled_capability_probe_server(None).await?;
    let (backup_url, backup_task) = start_capability_probe_server(None).await?;
    let test_dir = unique_test_dir("reload-validation-isolation");
    fs::create_dir_all(&test_dir)
        .with_context(|| format!("failed to create {}", test_dir.display()))?;
    let config_path = test_dir.join("watcher.toml");
    fs::write(
        &config_path,
        config_toml("mainnet-beta", &primary_url, &backup_url),
    )
    .with_context(|| format!("failed to write {}", config_path.display()))?;
    let mut runtime = RuntimeState::load(
        config_path.clone(),
        crate::watcher_state::RuntimeTelemetry::unobserved(),
    )?;

    primary_control.set_vote_accounts_failure(true);
    fs::write(
        &config_path,
        config_toml("testnet", &primary_url, &backup_url),
    )
    .with_context(|| format!("failed to write {}", config_path.display()))?;
    runtime.config_modified = UNIX_EPOCH;
    assert!(!runtime.reload_if_needed().await?);

    primary_control.set_vote_accounts_failure(false);
    let vote_accounts_source = runtime.sources.vote_accounts_source(Some(128)).await?;
    assert_eq!(vote_accounts_source.connector.as_ref(), "primary");

    fs::remove_dir_all(&test_dir)
        .with_context(|| format!("failed to remove directory {}", test_dir.display()))?;
    primary_task.abort();
    backup_task.abort();
    Ok(())
}

#[tokio::test]
async fn reload_if_needed_keeps_last_known_good_runtime_after_invalid_candidate() -> Result<()> {
    let test_dir = unique_test_dir("reload-invalid");
    fs::create_dir_all(&test_dir)
        .with_context(|| format!("failed to create {}", test_dir.display()))?;
    let config_path = test_dir.join("watcher.toml");
    fs::write(
        &config_path,
        r#"
network = "mainnet-beta"

[[connectors]]
name = "primary"
provider = "solana"
features = []
solana_rpc_compatibility = "extended"
rpc = { url = "https://primary.example" }

[modes.health.commands]
rpc_health = ["primary"]

[modes.consensus.commands]
epoch_info = ["primary"]
vote_accounts = ["primary"]
recent_performance_samples = ["primary"]

[modes.leadership.commands]
current_block_production = ["primary"]
current_leader_schedule = ["primary"]
next_leader_schedule = ["primary"]

[modes.economics]
enabled = false
"#,
    )
    .with_context(|| format!("failed to write {}", config_path.display()))?;

    let mut runtime = RuntimeState::load(
        config_path.clone(),
        crate::watcher_state::RuntimeTelemetry::unobserved(),
    )?;
    runtime.economics_capabilities_verified = true;
    runtime.config_modified = UNIX_EPOCH;

    fs::write(
        &config_path,
        r#"
network = "testnet"

[[connectors]]
name = "ws-only"
provider = "helius"
features = []
websocket = { url = "wss://ws-only.example" }
"#,
    )
    .with_context(|| format!("failed to write {}", config_path.display()))?;

    let error = match runtime.reload_if_needed().await {
        Ok(_) => anyhow::bail!("invalid reload candidate should fail"),
        Err(error) => error,
    };

    assert!(error
        .to_string()
        .contains("at least one connector with an rpc transport is required"));
    assert_eq!(runtime.config.network, "mainnet-beta");
    assert_eq!(runtime.config.connectors.len(), 1);
    assert_eq!(runtime.config.connectors[0].name, "primary");
    let rpc_health_route = runtime
        .sources
        .routes()
        .filter(|route| route.command == CommandId::RpcHealth)
        .map(|route| route.connector.to_string())
        .collect::<Vec<_>>();
    assert_eq!(rpc_health_route, ["primary"]);
    assert_eq!(runtime.config_modified, UNIX_EPOCH);
    assert!(runtime.economics_capabilities_verified);

    fs::remove_dir_all(&test_dir)
        .with_context(|| format!("failed to remove directory {}", test_dir.display()))?;
    Ok(())
}

#[tokio::test]
async fn reload_if_needed_preserves_runtime_when_candidate_capability_probe_fails() -> Result<()> {
    let (rpc_url, server_task) =
        start_capability_probe_server(Some("getRecentPerformanceSamples")).await?;
    let test_dir = unique_test_dir("reload-capability");
    fs::create_dir_all(&test_dir)
        .with_context(|| format!("failed to create {}", test_dir.display()))?;
    let config_path = test_dir.join("watcher.toml");
    fs::write(
        &config_path,
        r#"
network = "mainnet-beta"

[[connectors]]
name = "current"
provider = "solana"
features = []
solana_rpc_compatibility = "extended"
rpc = { url = "https://current.example" }

[modes.health.commands]
rpc_health = ["current"]

[modes.consensus.commands]
epoch_info = ["current"]
vote_accounts = ["current"]

[modes.economics]
enabled = false
"#,
    )
    .with_context(|| format!("failed to write {}", config_path.display()))?;

    let mut runtime = RuntimeState::load(
        config_path.clone(),
        crate::watcher_state::RuntimeTelemetry::unobserved(),
    )?;
    runtime.connector_capabilities_verified = true;
    runtime.economics_capabilities_verified = true;
    runtime.config_modified = UNIX_EPOCH;
    let secret = "do-not-expose-reload-key";
    fs::write(
        &config_path,
        format!(
            r#"
network = "testnet"

[[connectors]]
name = "candidate"
provider = "mock-provider"
features = []
solana_rpc_compatibility = "extended"
rpc = {{ url = "{rpc_url}/?api-key={secret}" }}

[modes.health.commands]
rpc_health = ["candidate"]

[modes.consensus.commands]
epoch_info = ["candidate"]
vote_accounts = ["candidate"]

[modes.economics]
enabled = false
"#
        ),
    )
    .with_context(|| format!("failed to write {}", config_path.display()))?;

    let Err(error) = runtime.reload_if_needed().await else {
        anyhow::bail!("unsupported reload candidate should fail");
    };
    let message = format!("{error:#}");
    assert!(message.contains("reload candidate"), "{message}");
    assert!(message.contains("connector \"candidate\""), "{message}");
    assert!(message.contains("getRecentPerformanceSamples"), "{message}");
    assert!(!message.contains(secret), "{message}");
    assert_eq!(runtime.config.network, "mainnet-beta");
    assert_eq!(runtime.config.connectors[0].name, "current");
    assert!(runtime.connector_capabilities_verified);
    assert_eq!(runtime.config_modified, UNIX_EPOCH);
    assert!(runtime.economics_capabilities_verified);
    let rpc_health_route = runtime
        .sources
        .routes()
        .filter(|route| route.command == CommandId::RpcHealth)
        .map(|route| route.connector.to_string())
        .collect::<Vec<_>>();
    assert_eq!(rpc_health_route, ["current"]);

    fs::remove_dir_all(&test_dir)
        .with_context(|| format!("failed to remove directory {}", test_dir.display()))?;
    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn reload_if_needed_preserves_runtime_when_candidate_economics_check_fails() -> Result<()> {
    let (rpc_url, server_task) = start_capability_probe_server(None).await?;
    let test_dir = unique_test_dir("reload-economics");
    fs::create_dir_all(&test_dir)
        .with_context(|| format!("failed to create {}", test_dir.display()))?;
    let config_path = test_dir.join("watcher.toml");
    fs::write(
        &config_path,
        r#"
network = "mainnet-beta"

[[connectors]]
name = "current"
provider = "solana"
features = []
solana_rpc_compatibility = "extended"
rpc = { url = "https://current.example" }

[modes.health.commands]
rpc_health = ["current"]

[modes.consensus.commands]
epoch_info = ["current"]
vote_accounts = ["current"]

[modes.economics]
enabled = false
"#,
    )
    .with_context(|| format!("failed to write {}", config_path.display()))?;

    let mut runtime = RuntimeState::load(
        config_path.clone(),
        crate::watcher_state::RuntimeTelemetry::unobserved(),
    )?;
    runtime.connector_capabilities_verified = true;
    runtime.economics_capabilities_verified = true;
    runtime.config_modified = UNIX_EPOCH;

    fs::write(
        &config_path,
        format!(
            r#"
network = "testnet"

[[connectors]]
name = "candidate"
provider = "mock-provider"
features = ["historical-data"]
solana_rpc_compatibility = "extended"
rpc = {{ url = "{rpc_url}" }}

[modes.health.commands]
rpc_health = ["candidate"]

[modes.consensus.commands]
epoch_info = ["candidate"]
vote_accounts = ["candidate"]

[modes.economics]
enabled = true

[modes.economics.validator]
identity = "missing-id"
vote_account = "missing-vote"
labels = []

[modes.economics.commands]
account_balances = ["candidate"]
inflation_rewards = ["candidate"]
reward_epoch_block_production = ["candidate"]
"#
        ),
    )
    .with_context(|| format!("failed to write {}", config_path.display()))?;

    let Err(error) = runtime.reload_if_needed().await else {
        anyhow::bail!("unsupported economics reload candidate should fail");
    };
    let message = format!("{error:#}");
    assert!(
        message.contains("reload candidate economics capability verification failed"),
        "{message}"
    );
    assert!(
        message.contains("configured economics validator is not present"),
        "{message}"
    );
    assert_eq!(runtime.config.network, "mainnet-beta");
    assert_eq!(runtime.config.connectors[0].name, "current");
    assert_eq!(runtime.config_modified, UNIX_EPOCH);
    assert!(runtime.connector_capabilities_verified);
    assert!(runtime.economics_capabilities_verified);
    let rpc_health_route = runtime
        .sources
        .routes()
        .filter(|route| route.command == CommandId::RpcHealth)
        .map(|route| route.connector.to_string())
        .collect::<Vec<_>>();
    assert_eq!(rpc_health_route, ["current"]);

    fs::remove_dir_all(&test_dir)
        .with_context(|| format!("failed to remove directory {}", test_dir.display()))?;
    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn reload_if_needed_rechecks_economics_when_root_inputs_change() -> Result<()> {
    let (rpc_url, server_task) = start_capability_probe_server(None).await?;
    let test_dir = unique_test_dir("reload-economics-input");
    fs::create_dir_all(&test_dir)
        .with_context(|| format!("failed to create {}", test_dir.display()))?;
    let config_path = test_dir.join("watcher.toml");
    let config_toml = |delinquent_slot_distance| {
        format!(
            r#"
network = "mainnet-beta"
delinquent_slot_distance = {delinquent_slot_distance}

[[connectors]]
name = "primary"
provider = "mock-provider"
features = ["historical-data"]
solana_rpc_compatibility = "extended"
rpc = {{ url = "{rpc_url}" }}

[modes.health.commands]
rpc_health = ["primary"]

[modes.consensus.commands]
epoch_info = ["primary"]
vote_accounts = ["primary"]

[modes.economics]
enabled = true

[modes.economics.validator]
identity = "id-1"
vote_account = "vote-1"
labels = []

[modes.economics.commands]
account_balances = ["primary"]
inflation_rewards = ["primary"]
reward_epoch_block_production = ["primary"]
"#
        )
    };
    fs::write(&config_path, config_toml(128))
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    let mut runtime = RuntimeState::load(
        config_path.clone(),
        crate::watcher_state::RuntimeTelemetry::unobserved(),
    )?;
    runtime.connector_capabilities_verified = true;
    runtime.economics_capabilities_verified = true;
    runtime.config_modified = UNIX_EPOCH;
    fs::write(&config_path, config_toml(256))
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    let Err(error) = runtime.reload_if_needed().await else {
        anyhow::bail!("unsupported economics input reload candidate should fail");
    };
    let message = format!("{error:#}");
    assert!(
        message.contains("reload candidate economics capability verification failed"),
        "{message}"
    );
    assert!(message.contains("getVoteAccounts"), "{message}");
    assert_eq!(runtime.config.delinquent_slot_distance, Some(128));
    assert_eq!(runtime.config_modified, UNIX_EPOCH);
    assert!(runtime.connector_capabilities_verified);
    assert!(runtime.economics_capabilities_verified);
    let rpc_health_route = runtime
        .sources
        .routes()
        .filter(|route| route.command == CommandId::RpcHealth)
        .map(|route| route.connector.to_string())
        .collect::<Vec<_>>();
    assert_eq!(rpc_health_route, ["primary"]);

    fs::remove_dir_all(&test_dir)
        .with_context(|| format!("failed to remove directory {}", test_dir.display()))?;
    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn reload_if_needed_rebuilds_rpc_client_when_authorization_header_changes() -> Result<()> {
    let (rpc_url, recorded_requests, server_task) =
        start_header_recording_capability_probe_server(None).await?;
    let test_dir = unique_test_dir("reload-rpc-header");
    fs::create_dir_all(&test_dir)
        .with_context(|| format!("failed to create {}", test_dir.display()))?;
    let config_path = test_dir.join("watcher.toml");
    let old_authorization = "Bearer old-token";
    let new_authorization = "Bearer new-token";
    let config_toml = |authorization: &str| {
        format!(
            r#"
network = "mainnet-beta"

[[connectors]]
name = "primary"
provider = "mock-provider"
features = []
solana_rpc_compatibility = "extended"
rpc = {{ url = "{rpc_url}", headers = {{ authorization = "{authorization}" }} }}

[modes.health.commands]
rpc_health = ["primary"]

[modes.consensus.commands]
epoch_info = ["primary"]
vote_accounts = ["primary"]

[modes.economics]
enabled = false
"#
        )
    };
    fs::write(&config_path, config_toml(old_authorization))
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    let mut runtime = RuntimeState::load(
        config_path.clone(),
        crate::watcher_state::RuntimeTelemetry::unobserved(),
    )?;
    runtime.config_modified = UNIX_EPOCH;

    fs::write(&config_path, config_toml(new_authorization))
        .with_context(|| format!("failed to write {}", config_path.display()))?;
    runtime.reload_if_needed().await?;

    {
        let requests = recorded_requests.lock().await;
        assert!(
            requests.iter().any(
                |(method, authorization)| method == "getRecentPerformanceSamples"
                    && authorization.as_deref() == Some(new_authorization)
            ),
            "reload must probe extended RPC capability with the replacement authorization header"
        );
        assert!(
            requests
                .iter()
                .all(|(_, authorization)| authorization.as_deref() == Some(new_authorization)),
            "reload capability probes must not use the previous authorization header: {requests:?}"
        );
    }

    let request_count_before_routed_health_check = recorded_requests.lock().await.len();
    let health = runtime.sources.rpc_health().await?;
    assert!(health.value.is_healthy());

    let requests = recorded_requests.lock().await;
    assert_eq!(
        requests.len(),
        request_count_before_routed_health_check + 1,
        "the routed health check should make one request"
    );
    assert_eq!(
        requests.last(),
        Some(&("getHealth".to_string(), Some(new_authorization.to_string()))),
        "the rebuilt client must send the replacement authorization header"
    );
    assert!(
        requests
            .iter()
            .all(|(_, authorization)| authorization.as_deref() == Some(new_authorization)),
        "no request may use the previous authorization header: {requests:?}"
    );
    assert!(
        !requests
            .iter()
            .any(|(_, authorization)| authorization.as_deref() == Some(old_authorization)),
        "no request may use the previous authorization header: {requests:?}"
    );
    drop(requests);

    fs::remove_dir_all(&test_dir)
        .with_context(|| format!("failed to remove directory {}", test_dir.display()))?;
    server_task.abort();
    let _ = server_task.await;
    Ok(())
}
