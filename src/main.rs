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
#[cfg(test)]
mod surfpool_tests;
mod tasks;
mod websocket;

use crate::{
    collector::{collect_snapshot, RpcClient},
    config::Config,
    metrics::Metrics,
    tasks::{Shutdown, TaskExit, TaskManager},
    websocket::WebsocketProbeClient,
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
    #[arg(long, default_value = "etc/config.local.yaml")]
    config: PathBuf,
}

#[derive(Clone)]
struct AppState {
    metrics: Arc<Metrics>,
}

struct RuntimeState {
    config_path: PathBuf,
    config: Config,
    client: RpcClient,
    ws_client: Option<WebsocketProbeClient>,
    config_modified: SystemTime,
    metrics_port: u16,
}

impl RuntimeState {
    fn load(config_path: PathBuf) -> Result<Self> {
        let config = Config::load(&config_path)?;
        let config_modified = config_mtime(&config_path)?;
        let metrics_port = config.metrics_port;
        let client = new_rpc_client(&config)?;
        let ws_client = new_ws_probe_client(&config);

        Ok(Self {
            config_path,
            config,
            client,
            ws_client,
            config_modified,
            metrics_port,
        })
    }

    fn reload_if_needed(&mut self) -> Result<()> {
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

        let rpc_settings_changed = self
            .config
            .validator_connection
            .rpc_settings_changed(&next.validator_connection);
        let ws_settings_changed = self
            .config
            .validator_connection
            .ws_settings_changed(&next.validator_connection);

        if rpc_settings_changed {
            self.client = new_rpc_client(&next)?;
        }
        if ws_settings_changed {
            self.ws_client = new_ws_probe_client(&next);
        }

        info!(
            watched_validators = next.watched_validators.len(),
            transport_mode = next.validator_connection.transport_mode.as_str(),
            validator_mode = next.validator_mode.as_str(),
            "reloaded configuration"
        );

        self.config = next;
        self.config_modified = modified;
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
    tasks.spawn("shutdown-signal", wait_for_shutdown_signal());

    tasks.run().await
}

async fn run_watcher_loop(
    mut runtime: RuntimeState,
    metrics: Arc<Metrics>,
    mut shutdown: Shutdown,
) -> Result<TaskExit> {
    info!(
        network = runtime.config.network,
        rpc_url = runtime.config.validator_connection.rpc_url,
        watched_validators = runtime.config.watched_validators.len(),
        transport_mode = runtime.config.validator_connection.transport_mode.as_str(),
        validator_mode = runtime.config.validator_mode.as_str(),
        "starting Solana validator watcher"
    );

    loop {
        if let Err(err) = runtime.reload_if_needed() {
            warn!(error = ?err, "failed to reload config, keeping previous configuration");
        }

        metrics.set_watcher_up(&runtime.config.network, true);
        metrics.update_runtime_config(&runtime.config);
        guard_dependencies_healthy(&runtime, metrics.as_ref()).await?;

        let snapshot = collect_snapshot(&runtime.client, &runtime.config)
            .await
            .with_context(|| {
                format!(
                    "failed to collect snapshot from RPC endpoint {}",
                    runtime.config.validator_connection.rpc_url
                )
            })?;
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

async fn guard_dependencies_healthy(runtime: &RuntimeState, metrics: &Metrics) -> Result<()> {
    metrics.set_rpc_up(&runtime.config.network, false);
    metrics.set_ws_up(&runtime.config.network, false);
    let rpc_url = runtime.config.validator_connection.rpc_url.clone();
    let ws_url = runtime.config.validator_connection.ws_url.clone();

    let rpc_check = async {
        runtime
            .client
            .ensure_healthy()
            .await
            .with_context(|| format!("RPC health guard failed for endpoint {}", rpc_url))
    };
    let ws_check = async {
        let Some(ws_client) = runtime.ws_client.as_ref() else {
            return Ok::<bool, anyhow::Error>(false);
        };
        let ws_url = ws_url
            .as_deref()
            .with_context(|| "missing ws_url for configured websocket probe")?;

        ws_client
            .probe()
            .await
            .with_context(|| format!("websocket health guard failed for endpoint {}", ws_url))?;
        Ok(true)
    };

    let (rpc_result, ws_result) = tokio::join!(rpc_check, ws_check);

    rpc_result?;
    metrics.set_rpc_up(&runtime.config.network, true);

    if ws_result? {
        metrics.set_ws_up(&runtime.config.network, true);
    }

    Ok(())
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

async fn wait_for_shutdown_signal() -> Result<TaskExit> {
    signal::ctrl_c()
        .await
        .context("failed to listen for shutdown signal")?;
    info!("received shutdown signal");
    Ok(TaskExit::ShutdownRequested)
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

fn new_rpc_client(config: &Config) -> Result<RpcClient> {
    let connection = &config.validator_connection;
    RpcClient::new(
        connection.rpc_url.clone(),
        Duration::from_secs(connection.rpc_timeout_sec),
        connection.commitment_config()?,
    )
}

fn new_ws_probe_client(config: &Config) -> Option<WebsocketProbeClient> {
    let connection = &config.validator_connection;
    connection.ws_url.as_ref().map(|url| {
        WebsocketProbeClient::new(url.clone(), Duration::from_secs(connection.ws_timeout_sec))
    })
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
        config_mtime, new_ws_probe_client, run_http_server, update_liveness_file, AppState,
    };
    use crate::{
        config::{Config, TransportMode, ValidatorConnectionConfig, ValidatorMode},
        metrics::Metrics,
        tasks::{TaskExit, TaskManager},
    };
    use anyhow::{Context, Result};
    use std::{
        env, fs,
        path::PathBuf,
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn new_ws_probe_client_respects_ws_configuration() {
        let with_ws = Config {
            validator_connection: ValidatorConnectionConfig {
                rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
                transport_mode: TransportMode::PollHttpAndWs,
                ws_url: Some("wss://api.mainnet-beta.solana.com".to_string()),
                ..ValidatorConnectionConfig::default()
            },
            validator_mode: ValidatorMode::ConsensusAndRpc,
            validator_websocket_enabled: true,
            ..Config::default()
        };
        let without_ws = Config {
            validator_connection: ValidatorConnectionConfig {
                ws_url: None,
                transport_mode: TransportMode::PollHttp,
                ..with_ws.validator_connection.clone()
            },
            validator_websocket_enabled: false,
            ..with_ws.clone()
        };

        assert!(new_ws_probe_client(&with_ws).is_some());
        assert!(new_ws_probe_client(&without_ws).is_none());
    }

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

    #[tokio::test]
    async fn http_server_task_stops_cleanly_when_shutdown_is_requested() -> Result<()> {
        let mut tasks = TaskManager::new();
        let state = AppState {
            metrics: Arc::new(Metrics::new()?),
        };

        tasks.spawn(
            "http-server",
            run_http_server(state, 0, tasks.shutdown_handle()),
        );
        tasks.spawn("signal", async move {
            tokio::task::yield_now().await;
            Ok(TaskExit::ShutdownRequested)
        });

        tasks.run().await?;
        Ok(())
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
