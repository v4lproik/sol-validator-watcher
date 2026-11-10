//! Single-writer runtime state for readiness and transport telemetry.
//!
//! Handles submit ordered events to the actor and read coherent snapshots without
//! exposing the actor's mutable state.

mod actor;
mod model;

pub(crate) use crate::mode::{RpcResponseCounts, WatcherStateSnapshot};
pub(crate) use actor::{
    WatcherState, WatcherStateError, WatcherStateMailbox, ADMISSION_SUCCESSFUL_POLL_ROUNDS,
};
pub(crate) use model::PollCompletion;
pub(crate) use sol_validator_watcher_source::RuntimeTelemetry;

#[cfg(test)]
mod tests;
