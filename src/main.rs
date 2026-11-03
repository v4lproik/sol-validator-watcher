#![deny(
    clippy::dbg_macro,
    clippy::expect_used,
    clippy::todo,
    clippy::unimplemented
)]
#![deny(clippy::unwrap_used)]

mod collector;
mod config;
mod labels;
mod metrics;
mod mode;
mod source;
#[cfg(test)]
mod surfpool_tests;
mod tasks;
mod websocket;

use crate::{
    collector::{
        collect_snapshot, command::GetHealth, ensure_economics_supported, ModeStatus,
        SnapshotDataSource,
    },
    config::Config,
    metrics::Metrics,
    mode::ModeId,
    source::SourceChain,
    tasks::{Shutdown, TaskExit, TaskManager},
};
use anyhow::{anyhow, Context, Result};
use axum::{extract::State, http::StatusCode, routing::get, Router};
use clap::Parser;
use std::{
    env, fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{net::TcpListener, signal, time::sleep};
use tracing::{error, info, warn};

#[derive(Parser, Debug)]
#[command(name = "sol-validator-watcher")]
#[command(about = "Prometheus watcher for Solana validators")]
struct Cli {
    #[arg(long, default_value = "config/local/validator.toml")]
    config: PathBuf,
}

#[derive(Clone)]
struct AppState {
    metrics: Arc<Metrics>,
}

struct RuntimeState {
    config_path: PathBuf,
    config: Config,
    sources: SourceChain,
    config_modified: SystemTime,
    metrics_port: u16,
    connector_capabilities_verified: bool,
    economics_capabilities_verified: bool,
}

impl RuntimeState {
    fn load(config_path: PathBuf) -> Result<Self> {
        let config = Config::load(&config_path)?;
        let config_modified = config_mtime(&config_path)?;
        let metrics_port = config.metrics_port;
        let sources = SourceChain::from_config(&config)?;

        Ok(Self {
            config_path,
            config,
            sources,
            config_modified,
            metrics_port,
            connector_capabilities_verified: false,
            economics_capabilities_verified: false,
        })
    }

    async fn reload_if_needed(&mut self) -> Result<()> {
        let modified = config_mtime(&self.config_path)?;
        if modified == self.config_modified {
            return Ok(());
        }

        let next = Config::load(&self.config_path)?;
        if next.metrics_port != self.metrics_port {
            warn!(
                old_metrics_port = self.metrics_port,
                new_metrics_port = next.metrics_port,
                "metrics_port changed in config; restart the watcher to move the HTTP listener"
            );
        }

        let sources_changed =
            self.config.connectors != next.connectors || self.config.modes != next.modes;
        let next_sources = if sources_changed {
            let sources = SourceChain::from_config(&next)?;
            sources
                .verify_capabilities()
                .await
                .context("reload candidate connector capability verification failed")?;
            Some(sources)
        } else {
            None
        };
        let candidate_sources = next_sources.as_ref().unwrap_or(&self.sources);
        ensure_economics_supported(candidate_sources, &next)
            .await
            .context("reload candidate economics capability verification failed")?;
        let next_economics_capabilities_verified = next.modes.economics.enabled;

        info!(
            watched_validators = next.watched_validators.len(),
            connectors = next.connectors.len(),
            economics_mode = next.modes.economics.as_str(),
            "reloaded configuration"
        );

        if let Some(sources) = next_sources {
            self.sources = sources;
            self.connector_capabilities_verified = true;
        }
        self.config = next;
        self.config_modified = modified;
        self.economics_capabilities_verified = next_economics_capabilities_verified;
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing()?;
    let cli = Cli::parse();
    let runtime = RuntimeState::load(cli.config)?;
    let metrics = Arc::new(Metrics::new()?);
    metrics.set_watcher_up(&runtime.config.network, true);
    metrics.set_rpc_up(&runtime.config.network, false);

    if let Err(err) = run(runtime, metrics).await {
        error!(error = ?err, "watcher exiting due to fatal error");
        return Err(err);
    }

    Ok(())
}

async fn run(runtime: RuntimeState, metrics: Arc<Metrics>) -> Result<()> {
    let server_state = AppState {
        metrics: Arc::clone(&metrics),
    };
    let mut tasks = TaskManager::new();

    tasks.spawn(
        "metrics-http-server",
        run_http_server(server_state, runtime.metrics_port, tasks.shutdown_handle()),
    );
    tasks.spawn(
        "watcher-loop",
        run_watcher_loop(runtime, metrics, tasks.shutdown_handle()),
    );
    tasks.spawn(
        "shutdown-signal",
        wait_for_shutdown_signal(tasks.shutdown_handle()),
    );

    tasks.run().await
}

async fn run_watcher_loop(
    mut runtime: RuntimeState,
    metrics: Arc<Metrics>,
    mut shutdown: Shutdown,
) -> Result<TaskExit> {
    info!(
        network = runtime.config.network,
        watched_validators = runtime.config.watched_validators.len(),
        connectors = runtime.config.connectors.len(),
        economics_mode = runtime.config.modes.economics.as_str(),
        "starting Solana validator watcher"
    );

    loop {
        if let Err(err) = runtime.reload_if_needed().await {
            warn!(error = ?err, "failed to reload config, keeping previous configuration");
        }
        ensure_connector_capabilities_ready(&mut runtime).await?;

        metrics.set_watcher_up(&runtime.config.network, true);
        metrics.begin_poll(&runtime.config, &runtime.sources);
        let (health_sources, health_status) =
            guard_dependencies_healthy(&runtime, metrics.as_ref()).await?;
        if let Err(error) = ensure_economics_capabilities_ready(&mut runtime).await {
            metrics.set_mode_up(&runtime.config.network, ModeId::Economics, false);
            return Err(error);
        }
        let mut snapshot = match collect_snapshot(&runtime.sources, &runtime.config).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                mark_collection_modes_down(&runtime.config, metrics.as_ref());
                return Err(error)
                    .context("failed to collect snapshot from configured command routes");
            }
        };
        snapshot.data_sources.extend(health_sources);
        snapshot.mode_statuses.push(health_status);
        if !runtime.config.watched_validators.is_empty() && snapshot.watched_validators.is_empty() {
            warn!(
                configured_watched_validators = runtime.config.watched_validators.len(),
                economics_mode = runtime.config.modes.economics.as_str(),
                "configured watched validators did not match any vote accounts in the latest snapshot; watched-validator panels will be empty"
            );
        }
        metrics.update_snapshot(&snapshot);
        update_liveness_file(runtime.config.liveness_file.as_deref())?;
        info!(
            epoch = snapshot.epoch,
            slot = snapshot.slot,
            scopes = snapshot.scopes.len(),
            "updated Prometheus metrics"
        );

        tokio::select! {
            _ = shutdown.cancelled() => {
                metrics.set_watcher_up(&runtime.config.network, false);
                info!("watcher loop shutting down");
                return Ok(TaskExit::Finished);
            }
            _ = sleep(Duration::from_secs(runtime.config.poll_interval_sec)) => {}
        }
    }
}

async fn guard_dependencies_healthy(
    runtime: &RuntimeState,
    metrics: &Metrics,
) -> Result<(Vec<SnapshotDataSource>, ModeStatus)> {
    metrics.set_rpc_up(&runtime.config.network, false);
    metrics.set_ws_up(&runtime.config.network, false);

    let rpc_health = match runtime.sources.required(&GetHealth).await {
        Ok(health) => health,
        Err(error) => {
            metrics.set_mode_up(&runtime.config.network, ModeId::Health, false);
            return Err(error).context("RPC health command failed for all configured connectors");
        }
    };
    metrics.set_rpc_up(&runtime.config.network, true);
    let mut sources = vec![rpc_health.source.into()];

    let health_up = match runtime.sources.websocket_healthy().await {
        Ok(Some(source)) => {
            metrics.set_ws_up(&runtime.config.network, true);
            sources.push(source.into());
            true
        }
        Ok(None) => true,
        Err(err) => {
            warn!(
                error = ?err,
                "websocket health command failed for all configured connectors"
            );
            false
        }
    };
    metrics.set_mode_up(&runtime.config.network, ModeId::Health, health_up);

    Ok((
        sources,
        ModeStatus {
            mode: ModeId::Health,
            up: health_up,
        },
    ))
}

fn mark_collection_modes_down(config: &Config, metrics: &Metrics) {
    for mode in [ModeId::Consensus, ModeId::Leadership, ModeId::Economics] {
        if config.modes.enabled(mode) {
            metrics.set_mode_up(&config.network, mode, false);
        }
    }
}

async fn ensure_connector_capabilities_ready(runtime: &mut RuntimeState) -> Result<()> {
    if runtime.connector_capabilities_verified {
        return Ok(());
    }

    info!("validating declared connector RPC capabilities");
    runtime
        .sources
        .verify_capabilities()
        .await
        .context("failed to validate declared connector RPC capabilities")?;
    runtime.connector_capabilities_verified = true;
    info!("validated declared connector RPC capabilities");
    Ok(())
}

async fn ensure_economics_capabilities_ready(runtime: &mut RuntimeState) -> Result<()> {
    if !economics_capability_check_required(runtime) {
        return Ok(());
    }

    info!(
        economics_mode = runtime.config.modes.economics.as_str(),
        "validating economics RPC capabilities"
    );

    ensure_economics_supported(&runtime.sources, &runtime.config)
        .await
        .context("failed to validate economics command routes")?;

    runtime.economics_capabilities_verified = true;
    info!("validated economics RPC capabilities");
    Ok(())
}

fn economics_capability_check_required(runtime: &RuntimeState) -> bool {
    !runtime.economics_capabilities_verified && runtime.config.modes.economics.enabled
}

async fn run_http_server(
    state: AppState,
    metrics_port: u16,
    mut shutdown: Shutdown,
) -> Result<TaskExit> {
    let address = SocketAddr::from(([0, 0, 0, 0], metrics_port));
    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/healthz", get(health_handler))
        .with_state(state);

    let listener = TcpListener::bind(address)
        .await
        .with_context(|| format!("failed to bind metrics server on {}", address))?;

    info!(address = %address, "serving metrics");
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown.cancelled().await;
        })
        .await
        .context("HTTP server failed")?;

    Ok(TaskExit::Finished)
}

async fn wait_for_shutdown_signal(mut shutdown: Shutdown) -> Result<TaskExit> {
    tokio::select! {
        result = signal::ctrl_c() => {
            result.context("failed to listen for shutdown signal")?;
            info!("received shutdown signal");
            Ok(TaskExit::ShutdownRequested)
        }
        _ = shutdown.cancelled() => Ok(TaskExit::Finished),
    }
}

async fn metrics_handler(
    State(state): State<AppState>,
) -> Result<(StatusCode, String), StatusCode> {
    state
        .metrics
        .render()
        .map(|body| (StatusCode::OK, body))
        .map_err(|err| {
            error!(error = ?err, "failed to render metrics");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn health_handler() -> &'static str {
    "ok"
}

fn config_mtime(path: &Path) -> Result<SystemTime> {
    fs::metadata(path)
        .with_context(|| format!("failed to stat config file {}", path.display()))?
        .modified()
        .with_context(|| format!("failed to read mtime for config file {}", path.display()))
}

fn update_liveness_file(path: Option<&Path>) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };

    let parent = path
        .parent()
        .with_context(|| format!("invalid liveness path {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "failed to create liveness parent directory {}",
            parent.display()
        )
    })?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before UNIX_EPOCH")?
        .as_secs()
        .to_string();
    fs::write(path, now)
        .with_context(|| format!("failed to update liveness file {}", path.display()))
}

fn init_tracing() -> Result<()> {
    const DEFAULT_LOG_FILTER: &str = "info,hyper=warn,h2=warn,tower_http=warn";
    let filter = match env::var(tracing_subscriber::EnvFilter::DEFAULT_ENV) {
        Ok(value) => tracing_subscriber::EnvFilter::try_new(value)
            .context("invalid RUST_LOG filter expression")?,
        Err(env::VarError::NotPresent) => {
            tracing_subscriber::EnvFilter::try_new(DEFAULT_LOG_FILTER)
                .context("failed to build default log filter")?
        }
        Err(err) => return Err(err).context("failed to read RUST_LOG"),
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .try_init()
        .map_err(|err| anyhow!("failed to initialize tracing subscriber: {err}"))
}

#[cfg(test)]
mod tests {
    use super::{
        config_mtime, economics_capability_check_required, ensure_economics_capabilities_ready,
        mark_collection_modes_down, run_watcher_loop, update_liveness_file, RuntimeState,
    };
    use crate::{
        config::{
            Config, ConnectorConfig, ConsensusCommands, ConsensusModeConfig, EconomicsModeConfig,
            HealthCommands, HealthModeConfig, LeadershipCommands, LeadershipModeConfig,
            ModesConfig, WatchedValidatorConfig,
        },
        metrics::Metrics,
        mode::{CommandId, ModeId, SolanaRpcCompatibility},
        source::SourceChain,
        tasks::TaskManager,
    };
    use anyhow::{Context, Result};
    use axum::{
        extract::{Json, State},
        routing::post,
        Router,
    };
    use serde_json::{json, Value};
    use std::{
        env, fs,
        path::PathBuf,
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };
    use tokio::{net::TcpListener, task::JoinHandle};

    #[test]
    fn update_liveness_file_writes_timestamp() -> Result<()> {
        let test_dir = unique_test_dir("liveness");
        fs::create_dir_all(&test_dir)
            .with_context(|| format!("failed to create {}", test_dir.display()))?;
        let path = test_dir.join("liveness.txt");
        update_liveness_file(Some(&path))?;

        let contents = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        assert!(!contents.trim().is_empty());

        fs::remove_dir_all(&test_dir)
            .with_context(|| format!("failed to remove directory {}", test_dir.display()))?;

        Ok(())
    }

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

    #[test]
    fn collection_failure_marks_enabled_collection_modes_down() -> Result<()> {
        let mut config = Config {
            network: "mainnet-beta".to_string(),
            ..Config::default()
        };
        config.modes.economics.enabled = true;
        let metrics = Metrics::new()?;
        for mode in ModeId::ALL {
            metrics.set_mode_up(&config.network, mode, true);
        }

        mark_collection_modes_down(&config, &metrics);

        let rendered = metrics.render()?;
        assert!(rendered.contains("sol_mode_up{mode=\"health\",network=\"mainnet-beta\"} 1"));
        assert!(rendered.contains("sol_mode_up{mode=\"consensus\",network=\"mainnet-beta\"} 0"));
        assert!(rendered.contains("sol_mode_up{mode=\"leadership\",network=\"mainnet-beta\"} 0"));
        assert!(rendered.contains("sol_mode_up{mode=\"economics\",network=\"mainnet-beta\"} 0"));
        Ok(())
    }

    #[test]
    fn economics_capability_check_required_depends_on_mode_and_verification_flag() -> Result<()> {
        let mut runtime = disabled_economics_runtime()?;
        assert!(!economics_capability_check_required(&runtime));

        runtime.config.modes.economics = EconomicsModeConfig {
            enabled: true,
            validator: Some(WatchedValidatorConfig {
                identity: Some("11111111111111111111111111111111".to_string()),
                vote_account: Some("vote-1".to_string()),
                labels: vec![],
            }),
            ..EconomicsModeConfig::default()
        };
        assert!(economics_capability_check_required(&runtime));

        runtime.economics_capabilities_verified = true;
        assert!(!economics_capability_check_required(&runtime));
        Ok(())
    }

    #[tokio::test]
    async fn ensure_economics_capabilities_ready_is_noop_when_disabled() -> Result<()> {
        let mut runtime = disabled_economics_runtime()?;

        ensure_economics_capabilities_ready(&mut runtime).await?;

        assert!(!runtime.economics_capabilities_verified);
        Ok(())
    }

    #[tokio::test]
    async fn ensure_economics_capabilities_ready_skips_preverified_runtime() -> Result<()> {
        let mut runtime = disabled_economics_runtime()?;
        runtime.config.modes.economics = EconomicsModeConfig {
            enabled: true,
            validator: Some(WatchedValidatorConfig {
                identity: Some("11111111111111111111111111111111".to_string()),
                vote_account: Some("vote-1".to_string()),
                labels: vec![],
            }),
            ..EconomicsModeConfig::default()
        };
        runtime.economics_capabilities_verified = true;

        ensure_economics_capabilities_ready(&mut runtime).await?;

        assert!(runtime.economics_capabilities_verified);
        Ok(())
    }

    #[tokio::test]
    async fn watcher_loop_fails_closed_before_publishing_capability_metrics() -> Result<()> {
        let (rpc_url, server_task) =
            start_capability_probe_server(Some("getRecentPerformanceSamples")).await?;
        let test_dir = unique_test_dir("startup-capability");
        fs::create_dir_all(&test_dir)
            .with_context(|| format!("failed to create {}", test_dir.display()))?;
        let config_path = test_dir.join("watcher.toml");
        let liveness_path = test_dir.join("watcher.liveness");
        let secret = "do-not-expose-startup-key";
        fs::write(
            &config_path,
            format!(
                r#"
network = "startup-test"
liveness_file = "{}"

[[connectors]]
name = "limited-rpc"
provider = "mock-provider"
features = []
solana_rpc_compatibility = "extended"
rpc_url = "{rpc_url}/?api-key={secret}"

[modes.health.commands]
rpc_health = ["limited-rpc"]

[modes.consensus.commands]
epoch_info = ["limited-rpc"]
vote_accounts = ["limited-rpc"]
recent_performance_samples = ["limited-rpc"]

[modes.economics]
enabled = false
"#,
                liveness_path.display()
            ),
        )
        .with_context(|| format!("failed to write {}", config_path.display()))?;

        let runtime = RuntimeState::load(config_path)?;
        let metrics = Arc::new(Metrics::new()?);
        let tasks = TaskManager::new();
        let Err(error) =
            run_watcher_loop(runtime, Arc::clone(&metrics), tasks.shutdown_handle()).await
        else {
            anyhow::bail!("unsupported startup connector must fail before collection");
        };
        let message = format!("{error:#}");
        assert!(message.contains("connector \"limited-rpc\""), "{message}");
        assert!(message.contains("getRecentPerformanceSamples"), "{message}");
        assert!(!message.contains(secret), "{message}");
        let rendered = metrics.render()?;
        assert!(
            !rendered.contains("sol_watcher_up{network=\"startup-test\""),
            "{rendered}"
        );
        assert!(
            !rendered.contains("sol_mode_command_route_info{"),
            "{rendered}"
        );
        assert!(!liveness_path.exists());

        fs::remove_dir_all(&test_dir)
            .with_context(|| format!("failed to remove directory {}", test_dir.display()))?;
        server_task.abort();
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
rpc_url = "https://primary.example"

[[connectors]]
name = "fallback"
provider = "helius"
features = []
ws_url = "wss://fallback.example"

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

        let mut runtime = RuntimeState::load(config_path.clone())?;
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
rpc_url = "{rpc_url}"
ws_url = "wss://fallback.example"

[[connectors]]
name = "replacement"
provider = "solana"
features = []
solana_rpc_compatibility = "extended"
rpc_url = "{rpc_url}"

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
    async fn reload_if_needed_keeps_last_known_good_runtime_after_invalid_candidate() -> Result<()>
    {
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
rpc_url = "https://primary.example"

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

        let mut runtime = RuntimeState::load(config_path.clone())?;
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
ws_url = "wss://ws-only.example"
"#,
        )
        .with_context(|| format!("failed to write {}", config_path.display()))?;

        let error = match runtime.reload_if_needed().await {
            Ok(()) => anyhow::bail!("invalid reload candidate should fail"),
            Err(error) => error,
        };

        assert!(error
            .to_string()
            .contains("at least one connector with rpc_url is required"));
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
    async fn reload_if_needed_preserves_runtime_when_candidate_capability_probe_fails() -> Result<()>
    {
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
rpc_url = "https://current.example"

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

        let mut runtime = RuntimeState::load(config_path.clone())?;
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
rpc_url = "{rpc_url}/?api-key={secret}"

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
    async fn reload_if_needed_preserves_runtime_when_candidate_economics_check_fails() -> Result<()>
    {
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
rpc_url = "https://current.example"

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

        let mut runtime = RuntimeState::load(config_path.clone())?;
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
rpc_url = "{rpc_url}"

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
rpc_url = "{rpc_url}"

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

        let mut runtime = RuntimeState::load(config_path.clone())?;
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

    fn disabled_economics_runtime() -> Result<RuntimeState> {
        let config = Config {
            connectors: vec![ConnectorConfig {
                name: "primary".to_string(),
                provider: "solana".to_string(),
                features: Vec::new(),
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
                rpc_url: Some("https://api.mainnet-beta.solana.com".to_string()),
                ..ConnectorConfig::default()
            }],
            modes: modes_for("primary"),
            ..Config::default()
        };
        let sources = SourceChain::from_config(&config)?;

        Ok(RuntimeState {
            config_path: PathBuf::from("config.toml"),
            config,
            sources,
            config_modified: UNIX_EPOCH,
            metrics_port: 8000,
            connector_capabilities_verified: false,
            economics_capabilities_verified: false,
        })
    }

    fn modes_for(connector: &str) -> ModesConfig {
        let route = || vec![connector.to_string()];
        ModesConfig {
            health: HealthModeConfig {
                commands: HealthCommands {
                    rpc_health: route(),
                    ws_health: vec![],
                },
            },
            consensus: ConsensusModeConfig {
                commands: ConsensusCommands {
                    epoch_info: route(),
                    vote_accounts: route(),
                    recent_performance_samples: route(),
                },
            },
            leadership: LeadershipModeConfig {
                commands: LeadershipCommands {
                    current_block_production: route(),
                    current_leader_schedule: route(),
                    next_leader_schedule: route(),
                },
            },
            economics: EconomicsModeConfig::default(),
        }
    }

    async fn start_capability_probe_server(
        missing_method: Option<&'static str>,
    ) -> Result<(String, JoinHandle<()>)> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .context("failed to bind capability probe listener")?;
        let address = listener
            .local_addr()
            .context("failed to read capability probe listener address")?;
        let app = Router::new()
            .route("/", post(capability_probe_handler))
            .with_state(missing_method);
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        Ok((format!("http://{address}"), task))
    }

    async fn capability_probe_handler(
        State(missing_method): State<Option<&'static str>>,
        Json(request): Json<Value>,
    ) -> Json<Value> {
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let params = request
            .get("params")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if method == "getVoteAccounts"
            && params
                .first()
                .and_then(|config| config.get("delinquentSlotDistance"))
                .and_then(Value::as_u64)
                == Some(256)
        {
            return Json(json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32602,
                    "message": "unsupported delinquent slot distance"
                },
                "id": 1
            }));
        }
        if missing_method == Some(method) {
            return Json(json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32601,
                    "message": "method not found"
                },
                "id": 1
            }));
        }

        let result = match method {
            "getHealth" => json!("ok"),
            "getEpochInfo" => json!({
                "epoch": 1,
                "slotIndex": 1,
                "slotsInEpoch": 32,
                "absoluteSlot": 33
            }),
            "getVoteAccounts" => json!({
                "current": [{
                    "votePubkey": "vote-1",
                    "nodePubkey": "id-1",
                    "activatedStake": 1_500_000_000u64,
                    "commission": 5,
                    "epochCredits": [[0, 20, 10]],
                    "lastVote": 32,
                    "rootSlot": 31
                }],
                "delinquent": []
            }),
            "getSlotLeader" => json!("probe-validator-identity"),
            "getLeaderSchedule" => Value::Null,
            "getRecentPerformanceSamples" => json!([]),
            "getBlockProduction" => json!({
                "context": { "slot": 33 },
                "value": {
                    "byIdentity": {},
                    "range": { "firstSlot": 32, "lastSlot": 33 }
                }
            }),
            "getMultipleAccounts" => json!({
                "context": { "slot": 33 },
                "value": [
                    { "lamports": 3_000_000_000u64, "space": 0 },
                    { "lamports": 2_500_000_000u64, "space": 3762 }
                ]
            }),
            "getMinimumBalanceForRentExemption" | "minimumLedgerSlot" => json!(0),
            "getInflationReward" => json!([null]),
            _ => {
                return Json(json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32601,
                        "message": "method not found"
                    },
                    "id": 1
                }));
            }
        };
        Json(json!({
            "jsonrpc": "2.0",
            "result": result,
            "id": 1
        }))
    }

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        env::temp_dir().join(format!(
            "sol-validator-watcher-{}-{}-{}",
            prefix,
            std::process::id(),
            nonce
        ))
    }
}
