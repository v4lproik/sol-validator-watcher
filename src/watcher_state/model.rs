use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RpcResponseCounts {
    pub total: u64,
    pub successful_2xx: u64,
    pub redirection_3xx: u64,
    pub client_error_4xx: u64,
    pub server_error_5xx: u64,
    pub other: u64,
    pub status_200: u64,
    pub status_403: u64,
    pub status_429: u64,
    pub status_500: u64,
    pub failed_attempts: u64,
}

impl RpcResponseCounts {
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

    pub(super) fn record_failure(&mut self) {
        self.failed_attempts = self.failed_attempts.saturating_add(1);
    }

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

#[derive(Clone, Debug)]
pub(crate) struct WatcherStateSnapshot {
    pub network: Arc<str>,
    pub completed_poll_rounds: u64,
    pub clean_poll_rounds: u8,
    pub ready: bool,
    pub rpc_responses: RpcResponseCounts,
}

impl WatcherStateSnapshot {
    pub(super) fn new(network: &str) -> Self {
        Self {
            network: Arc::from(network),
            completed_poll_rounds: 0,
            clean_poll_rounds: 0,
            ready: false,
            rpc_responses: RpcResponseCounts::default(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct PollCompletion {
    pub admitted: bool,
    pub state: WatcherStateSnapshot,
}
