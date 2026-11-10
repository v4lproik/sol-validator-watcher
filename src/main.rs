//! Solana validator watcher binary.
//!
//! This crate keeps `main` as the composition root: it parses the CLI, initializes
//! tracing, and delegates process ownership to the runtime module.

#![deny(
    clippy::dbg_macro,
    clippy::expect_used,
    clippy::todo,
    clippy::unimplemented
)]
#![deny(clippy::unwrap_used)]

mod collector;
mod config;
mod failover;
mod labels;
mod metrics;
mod mode;
mod runtime;
mod source;
#[cfg(test)]
mod surfpool_tests;
mod tasks;
mod watcher_state;
mod websocket;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use std::{env, net::SocketAddr, path::PathBuf};
use tracing::error;

#[derive(Parser, Debug)]
#[command(name = "sol-validator-watcher")]
#[command(about = "Prometheus watcher for Solana validators")]
struct Cli {
    #[arg(long, default_value = "config/local/validator.toml")]
    config: PathBuf,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Probe the process-liveness endpoint and exit.
    Healthcheck {
        /// Watcher HTTP address to probe.
        #[arg(long, default_value = "127.0.0.1:8000")]
        address: SocketAddr,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing()?;
    let cli = Cli::parse();
    if let Some(Command::Healthcheck { address }) = cli.command {
        return runtime::check_health(address).await;
    }
    if let Err(err) = runtime::run(cli.config).await {
        error!(error = ?err, "watcher exiting due to fatal error");
        return Err(err);
    }
    Ok(())
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
