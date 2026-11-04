//! Single-writer runtime state for readiness and transport telemetry.
//!
//! Handles submit ordered events to the actor and read coherent snapshots without
//! exposing the actor's mutable state.

mod actor;
mod model;

pub(crate) use actor::{
    ConnectorRpcTelemetry, ConnectorWebsocketTelemetry, RpcRequestOutcome, RpcTelemetry,
    WatcherState, WatcherStateError, WatcherStateMailbox, ADMISSION_CLEAN_POLL_ROUNDS,
};
pub(crate) use model::{PollCompletion, RpcResponseCounts, WatcherStateSnapshot};

#[cfg(test)]
mod tests;
