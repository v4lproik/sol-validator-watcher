//! Watcher-state actor, command handles, and periodic state reporting.
//!
//! The actor serializes poll transitions with transport outcomes so readiness and
//! exported counters describe one coherent event order.

use super::{PollCompletion, RpcResponseCounts, WatcherStateSnapshot};
use crate::{
    failover::{FailoverKey, FailoverStatus, FailoverTransition},
    metrics::Metrics,
    tasks::{Shutdown, TaskExit},
};
use anyhow::{anyhow, Result};
use std::{collections::BTreeMap, fmt, sync::Arc, time::Duration};
use tokio::{
    sync::{mpsc, oneshot, watch},
    time::{interval, MissedTickBehavior},
};
use tracing::info;

/// Consecutive successful polling rounds required before readiness admission.
pub(crate) const ADMISSION_SUCCESSFUL_POLL_ROUNDS: u8 = 5;
const WATCHER_STATE_REPORT_INTERVAL: Duration = Duration::from_secs(30);

/// Builder that creates all handles before transferring the receiver to the actor.
pub(crate) struct WatcherStateMailbox {
    commands: mpsc::UnboundedSender<WatcherStateCommand>,
    command_receiver: mpsc::UnboundedReceiver<WatcherStateCommand>,
}

/// Cloneable runtime event producer shared by source routing and connector clients.
#[derive(Clone)]
pub(crate) struct RuntimeTelemetry {
    commands: mpsc::UnboundedSender<WatcherStateCommand>,
    #[cfg(test)]
    _receiver_guard: Option<Arc<std::sync::Mutex<mpsc::UnboundedReceiver<WatcherStateCommand>>>>,
}

/// Connector-bound WebSocket failure producer preserving actor command ordering.
#[derive(Clone)]
pub(crate) struct ConnectorWebsocketTelemetry {
    commands: mpsc::UnboundedSender<WatcherStateCommand>,
    connector: Arc<str>,
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

/// Error returned while publishing a transition to the watcher-state actor.
#[derive(Debug)]
pub(crate) struct WatcherStateError {
    kind: WatcherStateErrorKind,
}

#[derive(Debug)]
enum WatcherStateErrorKind {
    ActorStopped(&'static str),
    InvalidTransition(StateTransitionError),
}

/// Single owner of readiness transitions and transport counters.
pub(crate) struct WatcherStateActor {
    commands: mpsc::UnboundedReceiver<WatcherStateCommand>,
    snapshots: watch::Sender<WatcherStateSnapshot>,
    state: WatcherStateSnapshot,
    poll: PollState,
    failovers: BTreeMap<FailoverKey, FailoverStatus>,
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
    RecordWebsocketProbeFailure {
        connector: Arc<str>,
    },
    RecordBlockFeeCache {
        connector: Arc<str>,
        hits: u64,
        misses: u64,
        methods_requested: u64,
    },
    RecordFailoverTransition {
        transition: FailoverTransition,
    },
    ResetAdmission {
        network: Arc<str>,
        reply: oneshot::Sender<()>,
    },
    ResetRuntime {
        network: Arc<str>,
        failovers: Vec<FailoverKey>,
        reply: oneshot::Sender<()>,
    },
    BeginPoll {
        reply: oneshot::Sender<Result<(), StateTransitionError>>,
    },
    CompletePoll {
        outcome: PollOutcome,
        reply: oneshot::Sender<Result<PollCompletion, StateTransitionError>>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PollState {
    Idle,
    Active,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PollOutcome {
    Succeeded,
    Failed,
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

impl WatcherStateError {
    fn actor_stopped(message: &'static str) -> Self {
        Self {
            kind: WatcherStateErrorKind::ActorStopped(message),
        }
    }

    fn invalid_transition(error: StateTransitionError) -> Self {
        Self {
            kind: WatcherStateErrorKind::InvalidTransition(error),
        }
    }

    /// Returns whether the actor stopped before acknowledging the transition.
    pub(crate) fn is_actor_stopped(&self) -> bool {
        matches!(self.kind, WatcherStateErrorKind::ActorStopped(_))
    }
}

impl fmt::Display for WatcherStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            WatcherStateErrorKind::ActorStopped(message) => formatter.write_str(message),
            WatcherStateErrorKind::InvalidTransition(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for WatcherStateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            WatcherStateErrorKind::ActorStopped(_) => None,
            WatcherStateErrorKind::InvalidTransition(error) => Some(error),
        }
    }
}

impl WatcherStateMailbox {
    /// Creates an unstarted mailbox.
    pub(crate) fn new() -> Self {
        let (commands, command_receiver) = mpsc::unbounded_channel();
        Self {
            commands,
            command_receiver,
        }
    }

    /// Returns source-routing and connector telemetry attached to this mailbox.
    pub(crate) fn runtime_telemetry(&self) -> RuntimeTelemetry {
        RuntimeTelemetry {
            commands: self.commands.clone(),
            #[cfg(test)]
            _receiver_guard: None,
        }
    }

    /// Initializes published state and consumes the mailbox into its actor and handle.
    pub(crate) fn start(
        self,
        network: &str,
        failover_inventory: &[FailoverKey],
        metrics: Arc<Metrics>,
    ) -> (WatcherState, WatcherStateActor) {
        let failovers = failover_inventory
            .iter()
            .cloned()
            .map(|key| (key, FailoverStatus::Primary))
            .collect::<BTreeMap<_, _>>();
        let state = WatcherStateSnapshot::new(network);
        metrics.reset_runtime_observations(network);
        metrics.update_watcher_state(&state, &failovers);
        let (snapshot_sender, snapshots) = watch::channel(state.clone());
        let actor = WatcherStateActor {
            commands: self.command_receiver,
            snapshots: snapshot_sender,
            state,
            poll: PollState::Idle,
            failovers,
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

impl RuntimeTelemetry {
    /// Binds RPC observations from this handle to one configured connector.
    pub(crate) fn for_connector(&self, connector: &str) -> ConnectorRpcTelemetry {
        ConnectorRpcTelemetry {
            commands: self.commands.clone(),
            connector: Arc::from(connector),
            #[cfg(test)]
            _receiver_guard: self._receiver_guard.clone(),
        }
    }

    /// Binds WebSocket observations from this handle to one configured connector.
    pub(crate) fn for_websocket_connector(&self, connector: &str) -> ConnectorWebsocketTelemetry {
        ConnectorWebsocketTelemetry {
            commands: self.commands.clone(),
            connector: Arc::from(connector),
            #[cfg(test)]
            _receiver_guard: self._receiver_guard.clone(),
        }
    }

    /// Queues one committed source failover transition.
    pub(crate) fn record_failover_transition(&self, transition: FailoverTransition) -> Result<()> {
        self.commands
            .send(WatcherStateCommand::RecordFailoverTransition { transition })
            .map_err(|_| {
                anyhow!("watcher state actor stopped before recording a failover transition")
            })
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
    /// Queues one received HTTP response status for bounded operator telemetry.
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

    /// Queues bounded finalized-block cache and JSON-RPC method counts.
    pub(crate) fn record_block_fee_cache(
        &self,
        hits: u64,
        misses: u64,
        methods_requested: u64,
    ) -> Result<()> {
        self.commands
            .send(WatcherStateCommand::RecordBlockFeeCache {
                connector: Arc::clone(&self.connector),
                hits,
                misses,
                methods_requested,
            })
            .map_err(|_| {
                anyhow!("watcher state actor stopped before recording block-fee cache telemetry")
            })
    }
}

impl ConnectorWebsocketTelemetry {
    /// Queues one failed WebSocket reachability probe.
    pub(crate) fn record_failure(&self) -> Result<()> {
        self.commands
            .send(WatcherStateCommand::RecordWebsocketProbeFailure {
                connector: Arc::clone(&self.connector),
            })
            .map_err(|_| {
                anyhow!("watcher state actor stopped before recording a websocket probe failure")
            })
    }
}

impl WatcherState {
    /// Resets readiness admission while preserving active source failover state.
    pub(crate) async fn reset_admission(
        &self,
        network: &str,
    ) -> std::result::Result<(), WatcherStateError> {
        let (reply, acknowledgment) = oneshot::channel();
        self.commands
            .send(WatcherStateCommand::ResetAdmission {
                network: Arc::from(network),
                reply,
            })
            .map_err(|_| {
                WatcherStateError::actor_stopped(
                    "watcher state actor stopped before admission reset",
                )
            })?;
        acknowledgment.await.map_err(|_| {
            WatcherStateError::actor_stopped(
                "watcher state actor stopped while resetting admission",
            )
        })
    }

    /// Resets admission and installs a newly compiled source failover inventory.
    pub(crate) async fn reset_runtime(
        &self,
        network: &str,
        failovers: &[FailoverKey],
    ) -> std::result::Result<(), WatcherStateError> {
        let (reply, acknowledgment) = oneshot::channel();
        self.commands
            .send(WatcherStateCommand::ResetRuntime {
                network: Arc::from(network),
                failovers: failovers.to_vec(),
                reply,
            })
            .map_err(|_| {
                WatcherStateError::actor_stopped("watcher state actor stopped before runtime reset")
            })?;
        acknowledgment.await.map_err(|_| {
            WatcherStateError::actor_stopped("watcher state actor stopped while resetting runtime")
        })
    }

    /// Opens a polling round before any RPC attempt is dispatched.
    pub(crate) async fn begin_poll(&self) -> std::result::Result<(), WatcherStateError> {
        let (reply, acknowledgment) = oneshot::channel();
        self.commands
            .send(WatcherStateCommand::BeginPoll { reply })
            .map_err(|_| {
                WatcherStateError::actor_stopped(
                    "watcher state actor stopped before beginning a poll",
                )
            })?;
        acknowledgment
            .await
            .map_err(|_| {
                WatcherStateError::actor_stopped(
                    "watcher state actor stopped while beginning a poll",
                )
            })?
            .map_err(WatcherStateError::invalid_transition)
    }

    /// Completes a successful active poll and advances readiness admission.
    pub(crate) async fn complete_poll(
        &self,
    ) -> std::result::Result<PollCompletion, WatcherStateError> {
        self.finish_poll(PollOutcome::Succeeded).await
    }

    /// Completes a failed active poll and resets readiness admission.
    pub(crate) async fn fail_poll(&self) -> std::result::Result<PollCompletion, WatcherStateError> {
        self.finish_poll(PollOutcome::Failed).await
    }

    async fn finish_poll(
        &self,
        outcome: PollOutcome,
    ) -> std::result::Result<PollCompletion, WatcherStateError> {
        let (reply, completion) = oneshot::channel();
        self.commands
            .send(WatcherStateCommand::CompletePoll { outcome, reply })
            .map_err(|_| {
                WatcherStateError::actor_stopped(
                    "watcher state actor stopped before poll completion",
                )
            })?;
        completion
            .await
            .map_err(|_| {
                WatcherStateError::actor_stopped(
                    "watcher state actor stopped while completing a poll",
                )
            })?
            .map_err(WatcherStateError::invalid_transition)
    }

    /// Returns the latest readiness, degradation, and successful-round state.
    pub(crate) fn readiness(&self) -> (bool, bool, u8) {
        let snapshot = self.snapshots.borrow();
        (
            snapshot.ready,
            snapshot.degraded,
            snapshot.successful_poll_rounds,
        )
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
            WatcherStateCommand::RecordWebsocketProbeFailure { connector } => {
                self.metrics
                    .record_ws_probe_failure(self.state.network.as_ref(), connector.as_ref());
            }
            WatcherStateCommand::RecordBlockFeeCache {
                connector,
                hits,
                misses,
                methods_requested,
            } => {
                self.metrics.record_block_fee_cache(
                    self.state.network.as_ref(),
                    connector.as_ref(),
                    hits,
                    misses,
                    methods_requested,
                );
            }
            WatcherStateCommand::RecordFailoverTransition { transition } => {
                info!(
                    network = self.state.network.as_ref(),
                    connector = transition.key.connector.as_ref(),
                    provider = transition.key.provider.as_ref(),
                    source_requirement = transition.key.source_requirement.as_str(),
                    failover_scope = transition.key.scope.as_str(),
                    command = transition
                        .key
                        .command
                        .map_or("", crate::mode::CommandId::as_str),
                    failover_state = transition.status.as_str(),
                    failover_transition = transition.kind.as_str(),
                    "source failover state changed"
                );
                self.metrics
                    .record_failover_transition(self.state.network.as_ref(), &transition);
                self.failovers.insert(transition.key, transition.status);
                self.state.degraded = self
                    .failovers
                    .values()
                    .copied()
                    .any(FailoverStatus::is_degraded);
                self.publish();
            }
            WatcherStateCommand::ResetAdmission { network, reply } => {
                self.state.network = network;
                self.state.completed_poll_rounds = 0;
                self.state.successful_poll_rounds = 0;
                self.state.ready = false;
                self.state.degraded = self
                    .failovers
                    .values()
                    .copied()
                    .any(FailoverStatus::is_degraded);
                self.poll = PollState::Idle;
                self.previous_responses = self.state.rpc_responses;
                self.metrics
                    .reset_runtime_observations(self.state.network.as_ref());
                self.publish();
                // The requester may be cancelled after the state transition commits.
                let _ = reply.send(());
            }
            WatcherStateCommand::ResetRuntime {
                network,
                failovers,
                reply,
            } => {
                self.state.network = network;
                self.state.completed_poll_rounds = 0;
                self.state.successful_poll_rounds = 0;
                self.state.ready = false;
                self.state.degraded = false;
                self.poll = PollState::Idle;
                self.failovers = failovers
                    .into_iter()
                    .map(|key| (key, FailoverStatus::Primary))
                    .collect();
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
                        self.poll = PollState::Active;
                        Ok(())
                    }
                    PollState::Active => Err(StateTransitionError::PollAlreadyActive),
                };
                // The requester may be cancelled after the state transition commits.
                let _ = reply.send(result);
            }
            WatcherStateCommand::CompletePoll { outcome, reply } => {
                let result = self.complete_poll(outcome);
                // The requester may be cancelled after the state transition commits.
                let _ = reply.send(result);
            }
        }
    }

    fn record_rpc_failure(&mut self, connector: &str, method: &str) {
        self.state.rpc_responses.record_failure();
        self.metrics
            .record_rpc_request_failure(self.state.network.as_ref(), connector, method);
    }

    fn complete_poll(
        &mut self,
        outcome: PollOutcome,
    ) -> Result<PollCompletion, StateTransitionError> {
        let PollState::Active = self.poll else {
            return Err(StateTransitionError::PollNotActive);
        };
        self.poll = PollState::Idle;
        self.state.completed_poll_rounds = self.state.completed_poll_rounds.saturating_add(1);
        self.state.successful_poll_rounds = if outcome == PollOutcome::Failed {
            0
        } else {
            self.state
                .successful_poll_rounds
                .saturating_add(1)
                .min(ADMISSION_SUCCESSFUL_POLL_ROUNDS)
        };
        let ready = self.state.successful_poll_rounds == ADMISSION_SUCCESSFUL_POLL_ROUNDS;
        let admitted = ready && !self.state.ready;
        self.state.ready = ready;
        self.publish();

        Ok(PollCompletion {
            admitted,
            state: self.state.clone(),
        })
    }

    fn publish(&self) {
        self.metrics
            .update_watcher_state(&self.state, &self.failovers);
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
            degraded = self.state.degraded,
            completed_poll_rounds = self.state.completed_poll_rounds,
            successful_poll_rounds = self.state.successful_poll_rounds,
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
