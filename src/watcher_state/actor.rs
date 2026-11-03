//! Watcher-state actor, command handles, and periodic state reporting.
//!
//! The actor serializes poll transitions with RPC outcomes so readiness and
//! exported counters describe one coherent event order.

use super::{PollCompletion, RpcResponseCounts, WatcherStateSnapshot};
use crate::{
    metrics::Metrics,
    tasks::{Shutdown, TaskExit},
};
use anyhow::{anyhow, Context, Result};
use std::{fmt, sync::Arc, time::Duration};
use tokio::{
    sync::{mpsc, oneshot, watch},
    time::{interval, MissedTickBehavior},
};
use tracing::info;

/// Consecutive clean polling rounds required before readiness admission.
pub(crate) const ADMISSION_CLEAN_POLL_ROUNDS: u8 = 5;
const WATCHER_STATE_REPORT_INTERVAL: Duration = Duration::from_secs(30);

/// Builder that creates all handles before transferring the receiver to the actor.
pub(crate) struct WatcherStateMailbox {
    commands: mpsc::UnboundedSender<WatcherStateCommand>,
    command_receiver: mpsc::UnboundedReceiver<WatcherStateCommand>,
}

/// Cloneable RPC event producer preserving actor command ordering.
#[derive(Clone)]
pub(crate) struct RpcTelemetry {
    commands: mpsc::UnboundedSender<WatcherStateCommand>,
    #[cfg(test)]
    _receiver_guard: Option<Arc<std::sync::Mutex<mpsc::UnboundedReceiver<WatcherStateCommand>>>>,
}

/// Final protocol outcome for one timed JSON-RPC request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RpcRequestOutcome {
    Succeeded,
    Failed,
}

/// Cloneable control and snapshot handle for watcher readiness.
#[derive(Clone)]
pub(crate) struct WatcherState {
    commands: mpsc::UnboundedSender<WatcherStateCommand>,
    snapshots: watch::Receiver<WatcherStateSnapshot>,
}

/// Single owner of readiness transitions and RPC response counters.
pub(crate) struct WatcherStateActor {
    commands: mpsc::UnboundedReceiver<WatcherStateCommand>,
    snapshots: watch::Sender<WatcherStateSnapshot>,
    state: WatcherStateSnapshot,
    poll: PollState,
    previous_responses: RpcResponseCounts,
    metrics: Arc<Metrics>,
}

enum WatcherStateCommand {
    RecordRpcResponse {
        status: u16,
    },
    RecordRpcRequest {
        connector: Arc<str>,
        method: &'static str,
        duration: Duration,
        outcome: RpcRequestOutcome,
    },
    RecordRpcFailure {
        connector: Arc<str>,
        method: &'static str,
    },
    ResetAdmission {
        network: Arc<str>,
        reply: oneshot::Sender<()>,
    },
    BeginPoll {
        reply: oneshot::Sender<Result<(), StateTransitionError>>,
    },
    CompletePoll {
        reply: oneshot::Sender<Result<PollCompletion, StateTransitionError>>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PollState {
    Idle,
    Active { dirty: bool },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StateTransitionError {
    PollAlreadyActive,
    PollNotActive,
}

impl fmt::Display for StateTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PollAlreadyActive => formatter.write_str("a polling round is already active"),
            Self::PollNotActive => formatter.write_str("no polling round is active"),
        }
    }
}

impl std::error::Error for StateTransitionError {}

impl WatcherStateMailbox {
    /// Creates an unstarted mailbox.
    pub(crate) fn new() -> Self {
        let (commands, command_receiver) = mpsc::unbounded_channel();
        Self {
            commands,
            command_receiver,
        }
    }

    /// Returns an RPC telemetry producer attached to this mailbox.
    pub(crate) fn rpc_telemetry(&self) -> RpcTelemetry {
        RpcTelemetry {
            commands: self.commands.clone(),
            #[cfg(test)]
            _receiver_guard: None,
        }
    }

    /// Initializes published state and consumes the mailbox into its actor and handle.
    pub(crate) fn start(
        self,
        network: &str,
        metrics: Arc<Metrics>,
    ) -> (WatcherState, WatcherStateActor) {
        let state = WatcherStateSnapshot::new(network);
        metrics.reset_runtime_observations(network);
        metrics.update_watcher_state(
            state.network.as_ref(),
            state.ready,
            state.completed_poll_rounds,
            state.clean_poll_rounds,
            state.rpc_responses,
        );
        let (snapshot_sender, snapshots) = watch::channel(state.clone());
        let actor = WatcherStateActor {
            commands: self.command_receiver,
            snapshots: snapshot_sender,
            state,
            poll: PollState::Idle,
            previous_responses: RpcResponseCounts::default(),
            metrics,
        };
        let handle = WatcherState {
            commands: self.commands,
            snapshots,
        };
        (handle, actor)
    }
}

impl RpcTelemetry {
    /// Binds request observations from this handle to one configured connector.
    pub(crate) fn for_connector(&self, connector: &str) -> ConnectorRpcTelemetry {
        ConnectorRpcTelemetry {
            commands: self.commands.clone(),
            connector: Arc::from(connector),
            #[cfg(test)]
            _receiver_guard: self._receiver_guard.clone(),
        }
    }

    #[cfg(test)]
    /// Queues one received HTTP response status for classification.
    pub(crate) fn record_response(&self, status: u16) -> Result<()> {
        self.commands
            .send(WatcherStateCommand::RecordRpcResponse { status })
            .map_err(|_| anyhow!("watcher state actor stopped before recording an RPC response"))
    }

    #[cfg(test)]
    pub(crate) fn unobserved() -> Self {
        let (commands, command_receiver) = mpsc::unbounded_channel();
        Self {
            commands,
            _receiver_guard: Some(Arc::new(std::sync::Mutex::new(command_receiver))),
        }
    }
}

/// Connector-bound RPC event producer preserving actor command ordering.
#[derive(Clone)]
pub(crate) struct ConnectorRpcTelemetry {
    commands: mpsc::UnboundedSender<WatcherStateCommand>,
    connector: Arc<str>,
    #[cfg(test)]
    _receiver_guard: Option<Arc<std::sync::Mutex<mpsc::UnboundedReceiver<WatcherStateCommand>>>>,
}

impl ConnectorRpcTelemetry {
    /// Queues one received HTTP response status for readiness classification.
    pub(crate) fn record_response(&self, status: u16) -> Result<()> {
        self.commands
            .send(WatcherStateCommand::RecordRpcResponse { status })
            .map_err(|_| anyhow!("watcher state actor stopped before recording an RPC response"))
    }

    /// Queues one timed request and its protocol outcome.
    pub(crate) fn record_request(
        &self,
        method: &'static str,
        duration: Duration,
        outcome: RpcRequestOutcome,
    ) -> Result<()> {
        self.commands
            .send(WatcherStateCommand::RecordRpcRequest {
                connector: Arc::clone(&self.connector),
                method,
                duration,
                outcome,
            })
            .map_err(|_| anyhow!("watcher state actor stopped before recording an RPC request"))
    }

    /// Queues one semantic failure discovered after a valid response was decoded.
    pub(crate) fn record_failure(&self, method: &'static str) -> Result<()> {
        self.commands
            .send(WatcherStateCommand::RecordRpcFailure {
                connector: Arc::clone(&self.connector),
                method,
            })
            .map_err(|_| anyhow!("watcher state actor stopped before recording an RPC failure"))
    }
}

impl WatcherState {
    /// Resets readiness admission and switches the active network atomically.
    pub(crate) async fn reset_admission(&self, network: &str) -> Result<()> {
        let (reply, acknowledgment) = oneshot::channel();
        self.commands
            .send(WatcherStateCommand::ResetAdmission {
                network: Arc::from(network),
                reply,
            })
            .map_err(|_| anyhow!("watcher state actor stopped before admission reset"))?;
        acknowledgment
            .await
            .context("watcher state actor stopped while resetting admission")
    }

    /// Opens a polling round before any RPC attempt is dispatched.
    pub(crate) async fn begin_poll(&self) -> Result<()> {
        let (reply, acknowledgment) = oneshot::channel();
        self.commands
            .send(WatcherStateCommand::BeginPoll { reply })
            .map_err(|_| anyhow!("watcher state actor stopped before beginning a poll"))?;
        acknowledgment
            .await
            .context("watcher state actor stopped while beginning a poll")??;
        Ok(())
    }

    /// Completes the active poll and returns the resulting admission transition.
    ///
    /// Any failed RPC attempt or non-2xx response recorded between `begin_poll`
    /// and this call resets the clean-round streak.
    pub(crate) async fn complete_poll(&self) -> Result<PollCompletion> {
        let (reply, completion) = oneshot::channel();
        self.commands
            .send(WatcherStateCommand::CompletePoll { reply })
            .map_err(|_| anyhow!("watcher state actor stopped before poll completion"))?;
        completion
            .await
            .context("watcher state actor stopped while completing a poll")?
            .map_err(Into::into)
    }

    /// Returns the latest readiness flag and clean-round streak.
    pub(crate) fn readiness(&self) -> (bool, u8) {
        let snapshot = self.snapshots.borrow();
        (snapshot.ready, snapshot.clean_poll_rounds)
    }

    #[cfg(test)]
    pub(crate) fn snapshot(&self) -> WatcherStateSnapshot {
        self.snapshots.borrow().clone()
    }
}

impl WatcherStateActor {
    /// Runs the single-owner state loop until shutdown or command-channel closure.
    pub(crate) async fn run(mut self, mut shutdown: Shutdown) -> Result<TaskExit> {
        let mut ticker = interval(WATCHER_STATE_REPORT_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        ticker.tick().await;

        loop {
            tokio::select! {
                command = self.commands.recv() => {
                    let Some(command) = command else {
                        info!("watcher state actor shutting down");
                        return Ok(TaskExit::Finished);
                    };
                    self.handle(command);
                }
                _ = ticker.tick() => self.report(),
                _ = shutdown.cancelled() => {
                    info!("watcher state actor shutting down");
                    return Ok(TaskExit::Finished);
                }
            }
        }
    }

    fn handle(&mut self, command: WatcherStateCommand) {
        match command {
            WatcherStateCommand::RecordRpcResponse { status } => {
                self.state.rpc_responses.record_response(status);
                if !(200..=299).contains(&status) {
                    self.mark_poll_dirty();
                }
            }
            WatcherStateCommand::RecordRpcRequest {
                connector,
                method,
                duration,
                outcome,
            } => {
                self.metrics.record_rpc_request_duration(
                    self.state.network.as_ref(),
                    connector.as_ref(),
                    method,
                    duration,
                );
                if outcome == RpcRequestOutcome::Failed {
                    self.record_rpc_failure(connector.as_ref(), method);
                }
            }
            WatcherStateCommand::RecordRpcFailure { connector, method } => {
                self.record_rpc_failure(connector.as_ref(), method);
            }
            WatcherStateCommand::ResetAdmission { network, reply } => {
                self.state.network = network;
                self.state.completed_poll_rounds = 0;
                self.state.clean_poll_rounds = 0;
                self.state.ready = false;
                self.poll = PollState::Idle;
                self.previous_responses = self.state.rpc_responses;
                self.metrics
                    .reset_runtime_observations(self.state.network.as_ref());
                self.publish();
                // The requester may be cancelled after the state transition commits.
                let _ = reply.send(());
            }
            WatcherStateCommand::BeginPoll { reply } => {
                let result = match self.poll {
                    PollState::Idle => {
                        self.poll = PollState::Active { dirty: false };
                        Ok(())
                    }
                    PollState::Active { .. } => Err(StateTransitionError::PollAlreadyActive),
                };
                // The requester may be cancelled after the state transition commits.
                let _ = reply.send(result);
            }
            WatcherStateCommand::CompletePoll { reply } => {
                let result = self.complete_poll();
                // The requester may be cancelled after the state transition commits.
                let _ = reply.send(result);
            }
        }
    }

    fn mark_poll_dirty(&mut self) {
        if let PollState::Active { dirty } = &mut self.poll {
            *dirty = true;
        }
    }

    fn record_rpc_failure(&mut self, connector: &str, method: &str) {
        self.state.rpc_responses.record_failure();
        self.metrics
            .record_rpc_request_failure(self.state.network.as_ref(), connector, method);
        self.mark_poll_dirty();
    }

    fn complete_poll(&mut self) -> Result<PollCompletion, StateTransitionError> {
        let PollState::Active { dirty } = self.poll else {
            return Err(StateTransitionError::PollNotActive);
        };
        self.poll = PollState::Idle;
        self.state.completed_poll_rounds = self.state.completed_poll_rounds.saturating_add(1);
        self.state.clean_poll_rounds = if dirty {
            0
        } else {
            self.state
                .clean_poll_rounds
                .saturating_add(1)
                .min(ADMISSION_CLEAN_POLL_ROUNDS)
        };
        let ready = self.state.clean_poll_rounds == ADMISSION_CLEAN_POLL_ROUNDS;
        let admitted = ready && !self.state.ready;
        self.state.ready = ready;
        self.publish();

        Ok(PollCompletion {
            admitted,
            state: self.state.clone(),
        })
    }

    fn publish(&self) {
        self.metrics.update_watcher_state(
            self.state.network.as_ref(),
            self.state.ready,
            self.state.completed_poll_rounds,
            self.state.clean_poll_rounds,
            self.state.rpc_responses,
        );
        self.snapshots.send_replace(self.state.clone());
    }

    fn report(&mut self) {
        let responses = self
            .state
            .rpc_responses
            .delta_since(self.previous_responses);
        self.previous_responses = self.state.rpc_responses;
        self.publish();

        info!(
            network = self.state.network.as_ref(),
            ready = self.state.ready,
            completed_poll_rounds = self.state.completed_poll_rounds,
            clean_poll_rounds = self.state.clean_poll_rounds,
            rpc_responses_30s = responses.total,
            rpc_responses_2xx_30s = responses.successful_2xx,
            rpc_responses_3xx_30s = responses.redirection_3xx,
            rpc_responses_4xx_30s = responses.client_error_4xx,
            rpc_responses_5xx_30s = responses.server_error_5xx,
            rpc_responses_other_30s = responses.other,
            rpc_status_200_30s = responses.status_200,
            rpc_status_403_30s = responses.status_403,
            rpc_status_429_30s = responses.status_429,
            rpc_status_500_30s = responses.status_500,
            rpc_failures_30s = responses.failed_attempts,
            rpc_failures_total = self.state.rpc_responses.failed_attempts,
            rpc_responses_total = self.state.rpc_responses.total,
            "watcher state"
        );
    }
}
