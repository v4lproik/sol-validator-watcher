//! Source provenance attached to collected command values.

use crate::{CommandId, ModeId, ProviderFeature, SolanaRpcCompatibility, Transport};
use std::sync::Arc;

/// Provenance for the connector that successfully supplied one command result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSelection {
    /// Mode that owns the command.
    pub mode: ModeId,
    /// Command that produced the value.
    pub command: CommandId,
    /// Stable configured connector name.
    pub connector: Arc<str>,
    /// Operator-declared provider name.
    pub provider: Arc<str>,
    /// Transport used by the command.
    pub transport: Transport,
    /// Standard Solana RPC profile declared by the connector.
    pub solana_rpc_compatibility: Option<SolanaRpcCompatibility>,
    /// Provider feature required by the command, if any.
    pub feature: Option<ProviderFeature>,
    /// One-based position of the connector in the command's route.
    pub precedence: usize,
}
