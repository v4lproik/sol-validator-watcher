//! Bounded runtime configuration inventory consumed by observability.

use crate::{CommandId, ModeId, ProviderFeature, SolanaRpcCompatibility, Transport};
use std::sync::Arc;

/// Prometheus-relevant runtime configuration compiled at startup or accepted reload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeInventory {
    /// Stable network label.
    pub network: Arc<str>,
    /// SHA-256 fingerprint of the fully expanded configuration.
    pub config_fingerprint: Arc<str>,
    /// Delay between completed polling rounds, in seconds.
    pub poll_interval_sec: u64,
    /// Whether any connector has a WebSocket transport.
    pub websocket_configured: bool,
    /// Connectors in declaration order.
    pub connectors: Vec<RuntimeConnectorInventory>,
    /// Collection-mode enablement in stable mode order.
    pub modes: Vec<RuntimeModeInventory>,
    /// Compiled command routes in command and precedence order.
    pub routes: Vec<RuntimeRouteInventory>,
}

/// One configured connector's bounded, non-secret metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeConnectorInventory {
    /// Stable configured connector name.
    pub name: Arc<str>,
    /// Operator-declared provider name.
    pub provider: Arc<str>,
    /// One-based declaration order.
    pub declaration_order: usize,
    /// Declared standard Solana RPC compatibility profile.
    pub solana_rpc_compatibility: Option<SolanaRpcCompatibility>,
    /// Whether an RPC transport is configured.
    pub has_rpc: bool,
    /// Whether a WebSocket transport is configured.
    pub has_websocket: bool,
    /// Whether a provider-specific API transport is configured.
    pub has_provider_api: bool,
    /// Declared provider features.
    pub features: Vec<ProviderFeature>,
}

/// Enablement state for one collection mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeModeInventory {
    /// Stable mode identifier.
    pub mode_id: ModeId,
    /// Whether this mode is enabled.
    pub enabled: bool,
}

/// One compiled command-route entry with bounded source metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeRouteInventory {
    /// Mode that owns the command.
    pub mode: ModeId,
    /// Routed command.
    pub command: CommandId,
    /// Stable configured connector name.
    pub connector: Arc<str>,
    /// Operator-declared provider name.
    pub provider: Arc<str>,
    /// Required command transport.
    pub transport: Transport,
    /// Solana RPC profile declared by the connector.
    pub solana_rpc_compatibility: Option<SolanaRpcCompatibility>,
    /// Provider feature required by the command, if any.
    pub feature: Option<ProviderFeature>,
    /// One-based fallback position.
    pub precedence: usize,
    /// Whether the connector satisfies the command's declared requirements.
    pub available: bool,
}
