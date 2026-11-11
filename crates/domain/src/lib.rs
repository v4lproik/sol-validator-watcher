//! Shared domain contracts for source collection, runtime state, and observability.

#![deny(
    clippy::dbg_macro,
    clippy::expect_used,
    clippy::todo,
    clippy::unimplemented,
    clippy::unwrap_used
)]

mod event;
mod failover;
mod inventory;
mod labels;
mod mode;
mod snapshot;
mod source;
mod state;

pub use event::*;
pub use failover::*;
pub use inventory::*;
pub use labels::*;
pub use mode::*;
pub use snapshot::*;
pub use source::*;
pub use state::*;
