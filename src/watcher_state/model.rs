//! Immutable watcher-state snapshots and bounded RPC response counters.
//!
//! Models contain no synchronization; mutation is owned by the watcher-state
//! actor.

use std::sync::Arc;

/// Saturating cumulative RPC response and failed-attempt counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RpcResponseCounts {
    /// Responses received with any HTTP status.
    pub total: u64,
    /// Responses with a 2xx status.
    pub successful_2xx: u64,
    /// Responses with a 3xx status.
    pub redirection_3xx: u64,
    /// Responses with a 4xx status.
    pub client_error_4xx: u64,
    /// Responses with a 5xx status.
    pub server_error_5xx: u64,
    /// Responses outside standard HTTP status classes.
    pub other: u64,
    /// Responses with status 200.
    pub status_200: u64,
    /// Responses with status 403.
    pub status_403: u64,
    /// Responses with status 429.
    pub status_429: u64,
    /// Responses with status 500.
    pub status_500: u64,
    /// RPC attempts that failed at transport, protocol, or semantic boundaries.
    pub failed_attempts: u64,
}

impl RpcResponseCounts {
    /// Records one received HTTP response in its class and selected exact-status buckets.
    pub(super) fn record_response(&mut self, status: u16) {
        self.total = self.total.saturating_add(1);
        match status {
            200..=299 => self.successful_2xx = self.successful_2xx.saturating_add(1),
            300..=399 => self.redirection_3xx = self.redirection_3xx.saturating_add(1),
            400..=499 => self.client_error_4xx = self.client_error_4xx.saturating_add(1),
            500..=599 => self.server_error_5xx = self.server_error_5xx.saturating_add(1),
            _ => self.other = self.other.saturating_add(1),
        }

        let exact_count = match status {
            200 => &mut self.status_200,
            403 => &mut self.status_403,
            429 => &mut self.status_429,
            500 => &mut self.status_500,
            _ => return,
        };
        *exact_count = exact_count.saturating_add(1);
    }

    /// Records one failed RPC attempt.
    pub(super) fn record_failure(&mut self) {
        self.failed_attempts = self.failed_attempts.saturating_add(1);
    }

    /// Returns the saturating field-wise delta from an earlier snapshot.
    pub(crate) fn delta_since(self, previous: Self) -> Self {
        Self {
            total: self.total.saturating_sub(previous.total),
            successful_2xx: self.successful_2xx.saturating_sub(previous.successful_2xx),
            redirection_3xx: self
                .redirection_3xx
                .saturating_sub(previous.redirection_3xx),
            client_error_4xx: self
                .client_error_4xx
                .saturating_sub(previous.client_error_4xx),
            server_error_5xx: self
                .server_error_5xx
                .saturating_sub(previous.server_error_5xx),
            other: self.other.saturating_sub(previous.other),
            status_200: self.status_200.saturating_sub(previous.status_200),
            status_403: self.status_403.saturating_sub(previous.status_403),
            status_429: self.status_429.saturating_sub(previous.status_429),
            status_500: self.status_500.saturating_sub(previous.status_500),
            failed_attempts: self
                .failed_attempts
                .saturating_sub(previous.failed_attempts),
        }
    }
}

/// Immutable state published by the watcher-state actor.
#[derive(Clone, Debug)]
pub(crate) struct WatcherStateSnapshot {
    /// Active network label.
    pub network: Arc<str>,
    /// Polling rounds completed for the active runtime.
    pub completed_poll_rounds: u64,
    /// Consecutive rounds that published a complete snapshot.
    pub successful_poll_rounds: u8,
    /// Whether any configured source failover circuit is open or probing.
    pub degraded: bool,
    /// Whether admission has reached the required successful-round streak.
    pub ready: bool,
    /// Cumulative process-lifetime RPC counters.
    pub rpc_responses: RpcResponseCounts,
}

impl WatcherStateSnapshot {
    /// Creates non-ready state for a network with zeroed counters.
    pub(super) fn new(network: &str) -> Self {
        Self {
            network: Arc::from(network),
            completed_poll_rounds: 0,
            successful_poll_rounds: 0,
            degraded: false,
            ready: false,
            rpc_responses: RpcResponseCounts::default(),
        }
    }
}

/// A completed poll transition and the state published with it.
#[derive(Debug)]
pub(crate) struct PollCompletion {
    /// Whether this transition newly admitted the watcher as ready.
    pub admitted: bool,
    /// State after applying the completed poll.
    pub state: WatcherStateSnapshot,
}
