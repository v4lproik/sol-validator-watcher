mod collector;
mod config;
mod labels;
mod metrics;
#[cfg(test)]
mod surfpool_tests;

use crate::{
    collector::{collect_snapshot, RpcClient},
    config::Config,
    metrics::Metrics,
};
use anyhow::{Context, Result};
use axum::{extract::State, http::StatusCode, routing::get, Router};
use clap::Parser;
use std::{
    fs,
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
    config_modified: SystemTime,
    metrics_port: u16,
}

impl RuntimeState {
    fn load(config_path: PathBuf) -> Result<Self> {
        let config = Config::load(&config_path)?;
        let config_modified = config_mtime(&config_path)?;
        let metrics_port = config.metrics_port;
        let client = new_rpc_client(&config)?;

        Ok(Self {
            config_path,
            config,
            client,
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

        let rpc_settings_changed = self.config.rpc_url != next.rpc_url
            || self.config.rpc_timeout_sec != next.rpc_timeout_sec
            || self.config.commitment != next.commitment;

        if rpc_settings_changed {
            self.client = new_rpc_client(&next)?;
        }

        info!(
            watched_validators = next.watched_validators.len(),
            "reloaded configuration"
        );

        self.config = next;
        self.config_modified = modified;
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();
    let runtime = RuntimeState::load(cli.config)?;
    let metrics = Arc::new(Metrics::new()?);
    metrics.set_watcher_up(&runtime.config.network, true);
    metrics.set_rpc_up(&runtime.config.network, false);

    let server_state = AppState {
        metrics: Arc::clone(&metrics),
    };
    let metrics_port = runtime.metrics_port;
    let server = tokio::spawn(async move {
        if let Err(err) = run_http_server(server_state, metrics_port).await {
            error!(error = ?err, "metrics HTTP server failed");
        }
    });

    let run_result = run_watcher_loop(runtime, Arc::clone(&metrics)).await;
    server.abort();
    run_result
}

async fn run_watcher_loop(mut runtime: RuntimeState, metrics: Arc<Metrics>) -> Result<()> {
    info!(
        network = runtime.config.network,
        rpc_url = runtime.config.rpc_url,
        watched_validators = runtime.config.watched_validators.len(),
        "starting Solana validator watcher"
    );

    loop {
        if let Err(err) = runtime.reload_if_needed() {
            warn!(error = ?err, "failed to reload config, keeping previous configuration");
        }

        metrics.set_watcher_up(&runtime.config.network, true);

        match collect_snapshot(&runtime.client, &runtime.config).await {
            Ok(snapshot) => {
                metrics.set_rpc_up(&snapshot.network, true);
                metrics.update_snapshot(&snapshot);
                update_liveness_file(runtime.config.liveness_file.as_deref())?;
                info!(
                    epoch = snapshot.epoch,
                    slot = snapshot.slot,
                    scopes = snapshot.scopes.len(),
                    "updated Prometheus metrics"
                );
            }
            Err(err) => {
                metrics.set_rpc_up(&runtime.config.network, false);
                warn!(error = ?err, "RPC collection failed");
            }
        }

        tokio::select! {
            _ = signal::ctrl_c() => {
                info!("received shutdown signal");
                break;
            }
            _ = sleep(Duration::from_secs(runtime.config.poll_interval_sec)) => {}
        }
    }

    Ok(())
}

async fn run_http_server(state: AppState, metrics_port: u16) -> Result<()> {
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
        .await
        .context("HTTP server failed")
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
    Ok(RpcClient::new(
        config.rpc_url.clone(),
        Duration::from_secs(config.rpc_timeout_sec),
        config.commitment_config()?,
    )?)
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

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,hyper=warn,h2=warn,tower_http=warn".into());

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .init();
}
