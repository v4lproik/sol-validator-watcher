//! Ordered observations emitted by source collection.

use crate::FailoverTransition;
use std::{sync::Arc, time::Duration};

/// Final protocol outcome for one timed JSON-RPC request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RpcRequestOutcome {
    /// The request and protocol response succeeded.
    Succeeded,
    /// The request or protocol response failed.
    Failed,
}

/// One source observation applied in collection order by the runtime actor.
#[derive(Clone, Debug, PartialEq)]
pub enum SourceEvent {
    /// One HTTP response received from an RPC endpoint.
    RpcResponse {
        /// HTTP status code.
        status: u16,
    },
    /// One timed RPC request and its final protocol outcome.
    RpcRequestCompleted {
        /// Stable configured connector name.
        connector: Arc<str>,
        /// Static JSON-RPC method name.
        method: &'static str,
        /// End-to-end request duration.
        duration: Duration,
        /// Final request outcome.
        outcome: RpcRequestOutcome,
    },
    /// One semantic failure discovered after decoding a valid RPC response.
    RpcFailure {
        /// Stable configured connector name.
        connector: Arc<str>,
        /// Static JSON-RPC method name.
        method: &'static str,
    },
    /// One failed WebSocket reachability probe.
    WebsocketProbeFailure {
        /// Stable configured connector name.
        connector: Arc<str>,
    },
    /// Bounded finalized-block cache and JSON-RPC method counts.
    BlockFeeCacheObserved {
        /// Stable configured connector name.
        connector: Arc<str>,
        /// Cache hits.
        hits: u64,
        /// Cache misses.
        misses: u64,
        /// JSON-RPC methods requested for cache misses.
        methods_requested: u64,
    },
    /// One committed source failover transition.
    FailoverTransition(FailoverTransition),
}
