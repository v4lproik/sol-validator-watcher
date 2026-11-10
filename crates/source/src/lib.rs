//! Typed source collection and source-chain routing.

#![deny(
    clippy::dbg_macro,
    clippy::expect_used,
    clippy::todo,
    clippy::unimplemented,
    clippy::unwrap_used
)]

use sol_validator_watcher_config as config;
use sol_validator_watcher_domain as mode;

pub mod collector;
mod failover;
mod labels;
pub mod source;
mod telemetry;
mod transport;
mod websocket;

pub use collector::{collect_snapshot, ensure_economics_supported, RpcClient};
pub use source::{SourceChain, SourceRoute, Sourced};
pub use telemetry::{
    source_telemetry_channel, ConnectorRpcTelemetry, ConnectorWebsocketTelemetry, RuntimeTelemetry,
    SourceTelemetryDelivery, SourceTelemetryError, SourceTelemetryReceiver,
};
