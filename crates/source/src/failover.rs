//! Classified connector failures and hierarchical source failover circuits.
//!
//! Transport failures open a source-requirement circuit shared by compatible
//! commands. Protocol and semantic failures open only the command circuit.

pub(crate) use crate::mode::{
    FailoverKey, FailoverScope, FailoverStatus, FailoverTransition, FailoverTransitionKind,
};
use anyhow::Error;
use std::{
    error::Error as StdError,
    fmt,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

/// Marker preserving that a failure occurred before a valid provider response.
#[derive(Debug)]
struct SourceRequirementFailure {
    source: Error,
}

impl fmt::Display for SourceRequirementFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.source, formatter)
    }
}

impl StdError for SourceRequirementFailure {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(self.source.as_ref())
    }
}

/// Tags a transport or connectivity error for source-requirement failover.
pub(crate) fn tag_source_requirement_failure(error: impl Into<Error>) -> Error {
    SourceRequirementFailure {
        source: error.into(),
    }
    .into()
}

/// Classifies an adapter error for hierarchical failover.
pub(crate) fn classify_failure(error: &Error) -> FailoverScope {
    if error.downcast_ref::<SourceRequirementFailure>().is_some() {
        FailoverScope::SourceRequirement
    } else {
        FailoverScope::Command
    }
}

/// Indices of the shared and command-specific circuits guarding one route entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FailoverGateIndices {
    pub(crate) source_requirement: usize,
    pub(crate) command: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CircuitAttempt {
    generation: u64,
    recovery_probe: bool,
}

/// Generation-tagged permission for one connector command attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FailoverAttempt {
    source_requirement: CircuitAttempt,
    command: CircuitAttempt,
}

impl FailoverAttempt {
    pub(crate) const fn ungated() -> Self {
        let attempt = CircuitAttempt {
            generation: 0,
            recovery_probe: false,
        };
        Self {
            source_requirement: attempt,
            command: attempt,
        }
    }

    pub(crate) const fn is_recovery_probe(self) -> bool {
        self.source_requirement.recovery_probe || self.command.recovery_probe
    }
}

/// Admission decision for one primary connector attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FailoverAttemptDecision {
    Attempt(FailoverAttempt),
    Skip,
}

/// Circuit-level transition before connector and command identity are attached.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CircuitTransition {
    pub(crate) scope: FailoverScope,
    pub(crate) status: FailoverStatus,
    pub(crate) kind: FailoverTransitionKind,
}

pub(crate) type CircuitTransitions = [Option<CircuitTransition>; 2];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CircuitState {
    Primary { generation: u64 },
    Open { generation: u64, retry_at: Instant },
    RecoveryProbe { generation: u64 },
}

impl CircuitState {
    const fn primary() -> Self {
        Self::Primary { generation: 0 }
    }

    const fn generation(self) -> u64 {
        match self {
            Self::Primary { generation }
            | Self::Open { generation, .. }
            | Self::RecoveryProbe { generation } => generation,
        }
    }

    fn blocks_attempt(self, now: Instant) -> bool {
        match self {
            Self::Open { retry_at, .. } => now < retry_at,
            Self::RecoveryProbe { .. } => true,
            Self::Primary { .. } => false,
        }
    }

    const fn recovery_due(self) -> bool {
        matches!(self, Self::Open { .. })
    }
}

struct FailoverStates {
    source_requirements: Vec<CircuitState>,
    commands: Vec<CircuitState>,
}

/// Atomic hierarchical circuit state for all compiled source routes.
pub(crate) struct FailoverController {
    cooldown: Duration,
    #[cfg(test)]
    source_requirement_count: usize,
    #[cfg(test)]
    command_count: usize,
    states: Mutex<FailoverStates>,
}

impl FailoverController {
    pub(crate) fn new(
        source_requirement_count: usize,
        command_count: usize,
        cooldown: Duration,
    ) -> Self {
        Self {
            cooldown,
            #[cfg(test)]
            source_requirement_count,
            #[cfg(test)]
            command_count,
            states: Mutex::new(FailoverStates {
                source_requirements: vec![CircuitState::primary(); source_requirement_count],
                commands: vec![CircuitState::primary(); command_count],
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_cooldown(&self, cooldown: Duration) -> Self {
        Self::new(self.source_requirement_count, self.command_count, cooldown)
    }

    pub(crate) async fn begin_attempt(
        &self,
        gates: FailoverGateIndices,
    ) -> (FailoverAttemptDecision, CircuitTransitions) {
        self.begin_attempt_at(gates, Instant::now()).await
    }

    async fn begin_attempt_at(
        &self,
        gates: FailoverGateIndices,
        now: Instant,
    ) -> (FailoverAttemptDecision, CircuitTransitions) {
        let mut states = self.states.lock().await;
        let source_requirement = states.source_requirements[gates.source_requirement];
        let command = states.commands[gates.command];

        if source_requirement.blocks_attempt(now) || command.blocks_attempt(now) {
            return (FailoverAttemptDecision::Skip, [None, None]);
        }

        let source_requirement_probe = source_requirement.recovery_due();
        let command_probe = command.recovery_due();
        let attempt = FailoverAttempt {
            source_requirement: CircuitAttempt {
                generation: source_requirement.generation(),
                recovery_probe: source_requirement_probe,
            },
            command: CircuitAttempt {
                generation: command.generation(),
                recovery_probe: command_probe,
            },
        };
        let mut transitions = [None, None];
        if source_requirement_probe {
            states.source_requirements[gates.source_requirement] = CircuitState::RecoveryProbe {
                generation: source_requirement.generation(),
            };
            transitions[0] = Some(CircuitTransition {
                scope: FailoverScope::SourceRequirement,
                status: FailoverStatus::RecoveryProbe,
                kind: FailoverTransitionKind::RecoveryProbeStarted,
            });
        }
        if command_probe {
            states.commands[gates.command] = CircuitState::RecoveryProbe {
                generation: command.generation(),
            };
            transitions[1] = Some(CircuitTransition {
                scope: FailoverScope::Command,
                status: FailoverStatus::RecoveryProbe,
                kind: FailoverTransitionKind::RecoveryProbeStarted,
            });
        }

        (FailoverAttemptDecision::Attempt(attempt), transitions)
    }

    pub(crate) async fn record_success(
        &self,
        gates: FailoverGateIndices,
        attempt: FailoverAttempt,
    ) -> CircuitTransitions {
        let mut states = self.states.lock().await;
        let source_requirement = recover(
            &mut states.source_requirements[gates.source_requirement],
            attempt.source_requirement,
        )
        .map(|kind| CircuitTransition {
            scope: FailoverScope::SourceRequirement,
            status: FailoverStatus::Primary,
            kind,
        });
        let command = recover(&mut states.commands[gates.command], attempt.command).map(|kind| {
            CircuitTransition {
                scope: FailoverScope::Command,
                status: FailoverStatus::Primary,
                kind,
            }
        });
        [source_requirement, command]
    }

    pub(crate) async fn record_failure(
        &self,
        gates: FailoverGateIndices,
        attempt: FailoverAttempt,
        failure_scope: FailoverScope,
    ) -> CircuitTransitions {
        self.record_failure_at(gates, attempt, failure_scope, Instant::now())
            .await
    }

    async fn record_failure_at(
        &self,
        gates: FailoverGateIndices,
        attempt: FailoverAttempt,
        failure_scope: FailoverScope,
        now: Instant,
    ) -> CircuitTransitions {
        let mut states = self.states.lock().await;
        let mut transitions = [None, None];

        match failure_scope {
            FailoverScope::SourceRequirement => {
                transitions[0] = open(
                    &mut states.source_requirements[gates.source_requirement],
                    attempt.source_requirement,
                    now,
                    self.cooldown,
                )
                .map(|kind| CircuitTransition {
                    scope: FailoverScope::SourceRequirement,
                    status: FailoverStatus::Open,
                    kind,
                });
                if attempt.command.recovery_probe {
                    transitions[1] = open(
                        &mut states.commands[gates.command],
                        attempt.command,
                        now,
                        self.cooldown,
                    )
                    .map(|kind| CircuitTransition {
                        scope: FailoverScope::Command,
                        status: FailoverStatus::Open,
                        kind,
                    });
                }
            }
            FailoverScope::Command => {
                if attempt.source_requirement.recovery_probe {
                    transitions[0] = recover(
                        &mut states.source_requirements[gates.source_requirement],
                        attempt.source_requirement,
                    )
                    .map(|kind| CircuitTransition {
                        scope: FailoverScope::SourceRequirement,
                        status: FailoverStatus::Primary,
                        kind,
                    });
                }
                transitions[1] = open(
                    &mut states.commands[gates.command],
                    attempt.command,
                    now,
                    self.cooldown,
                )
                .map(|kind| CircuitTransition {
                    scope: FailoverScope::Command,
                    status: FailoverStatus::Open,
                    kind,
                });
            }
        }

        transitions
    }
}

fn recover(state: &mut CircuitState, attempt: CircuitAttempt) -> Option<FailoverTransitionKind> {
    if !attempt.recovery_probe {
        return None;
    }
    if matches!(
        *state,
        CircuitState::RecoveryProbe { generation } if generation == attempt.generation
    ) {
        *state = CircuitState::Primary {
            generation: attempt.generation.wrapping_add(1),
        };
        Some(FailoverTransitionKind::Recovered)
    } else {
        None
    }
}

fn open(
    state: &mut CircuitState,
    attempt: CircuitAttempt,
    now: Instant,
    cooldown: Duration,
) -> Option<FailoverTransitionKind> {
    let kind = match (*state, attempt.recovery_probe) {
        (CircuitState::Primary { generation }, false) if generation == attempt.generation => {
            FailoverTransitionKind::Activated
        }
        (CircuitState::RecoveryProbe { generation }, true) if generation == attempt.generation => {
            FailoverTransitionKind::RecoveryProbeFailed
        }
        _ => return None,
    };
    *state = CircuitState::Open {
        generation: attempt.generation,
        retry_at: now + cooldown,
    };
    Some(kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    const FIRST: FailoverGateIndices = FailoverGateIndices {
        source_requirement: 0,
        command: 0,
    };
    const RELATED: FailoverGateIndices = FailoverGateIndices {
        source_requirement: 0,
        command: 1,
    };

    #[test]
    fn source_requirement_marker_survives_context() {
        let error = tag_source_requirement_failure(io::Error::other("offline"))
            .context("getEpochInfo request failed");
        assert_eq!(classify_failure(&error), FailoverScope::SourceRequirement);
        assert_eq!(
            classify_failure(&anyhow::anyhow!("invalid epoch payload")),
            FailoverScope::Command
        );
    }

    #[test]
    fn failover_labels_and_metric_values_match_the_operator_contract() {
        assert_eq!(
            FailoverScope::SourceRequirement.as_str(),
            "source-requirement"
        );
        assert_eq!(FailoverScope::Command.as_str(), "command");
        assert_eq!(FailoverStatus::Primary.as_str(), "primary");
        assert_eq!(FailoverStatus::Open.as_str(), "open");
        assert_eq!(FailoverStatus::RecoveryProbe.as_str(), "recovery-probe");
        assert_eq!(FailoverStatus::Primary.metric_value(), 0);
        assert_eq!(FailoverStatus::Open.metric_value(), 1);
        assert_eq!(FailoverStatus::RecoveryProbe.metric_value(), 2);
        assert_eq!(FailoverTransitionKind::Activated.as_str(), "activated");
        assert_eq!(
            FailoverTransitionKind::RecoveryProbeStarted.as_str(),
            "recovery-probe-started"
        );
        assert_eq!(
            FailoverTransitionKind::RecoveryProbeFailed.as_str(),
            "recovery-probe-failed"
        );
        assert_eq!(FailoverTransitionKind::Recovered.as_str(), "recovered");
    }

    #[tokio::test]
    async fn command_failure_isolated_from_related_command() {
        let controller = FailoverController::new(1, 2, Duration::from_secs(60));
        let started_at = Instant::now();
        let (FailoverAttemptDecision::Attempt(first), _) =
            controller.begin_attempt_at(FIRST, started_at).await
        else {
            panic!("primary command should be attempted");
        };
        let transitions = controller
            .record_failure_at(FIRST, first, FailoverScope::Command, started_at)
            .await;
        assert_eq!(
            transitions[1],
            Some(CircuitTransition {
                scope: FailoverScope::Command,
                status: FailoverStatus::Open,
                kind: FailoverTransitionKind::Activated,
            })
        );
        assert!(matches!(
            controller.begin_attempt_at(FIRST, started_at).await.0,
            FailoverAttemptDecision::Skip
        ));
        assert!(matches!(
            controller.begin_attempt_at(RELATED, started_at).await.0,
            FailoverAttemptDecision::Attempt(_)
        ));
    }

    #[tokio::test]
    async fn related_command_can_recover_source_requirement() {
        let cooldown = Duration::from_secs(60);
        let controller = FailoverController::new(1, 2, cooldown);
        let started_at = Instant::now();
        let (FailoverAttemptDecision::Attempt(first), _) =
            controller.begin_attempt_at(FIRST, started_at).await
        else {
            panic!("primary command should be attempted");
        };
        controller
            .record_failure_at(FIRST, first, FailoverScope::SourceRequirement, started_at)
            .await;
        assert!(matches!(
            controller.begin_attempt_at(RELATED, started_at).await.0,
            FailoverAttemptDecision::Skip
        ));

        let (FailoverAttemptDecision::Attempt(probe), transitions) = controller
            .begin_attempt_at(RELATED, started_at + cooldown)
            .await
        else {
            panic!("related command should probe recovered transport");
        };
        assert_eq!(
            transitions[0],
            Some(CircuitTransition {
                scope: FailoverScope::SourceRequirement,
                status: FailoverStatus::RecoveryProbe,
                kind: FailoverTransitionKind::RecoveryProbeStarted,
            })
        );
        assert!(matches!(
            controller
                .begin_attempt_at(FIRST, started_at + cooldown)
                .await
                .0,
            FailoverAttemptDecision::Skip
        ));
        let recovered = controller.record_success(RELATED, probe).await;
        assert_eq!(
            recovered[0].map(|transition| transition.status),
            Some(FailoverStatus::Primary)
        );
        assert!(matches!(
            controller
                .begin_attempt_at(FIRST, started_at + cooldown)
                .await
                .0,
            FailoverAttemptDecision::Attempt(_)
        ));
    }

    #[tokio::test]
    async fn command_error_during_transport_probe_recovers_only_transport() {
        let controller = FailoverController::new(1, 2, Duration::ZERO);
        let now = Instant::now();
        let (FailoverAttemptDecision::Attempt(first), _) =
            controller.begin_attempt_at(FIRST, now).await
        else {
            panic!("primary command should be attempted");
        };
        controller
            .record_failure_at(FIRST, first, FailoverScope::SourceRequirement, now)
            .await;
        let (FailoverAttemptDecision::Attempt(probe), _) =
            controller.begin_attempt_at(RELATED, now).await
        else {
            panic!("related command should probe transport");
        };
        let transitions = controller
            .record_failure_at(RELATED, probe, FailoverScope::Command, now)
            .await;
        assert_eq!(
            transitions[0].map(|transition| transition.status),
            Some(FailoverStatus::Primary)
        );
        assert_eq!(
            transitions[1].map(|transition| transition.status),
            Some(FailoverStatus::Open)
        );
        assert!(matches!(
            controller.begin_attempt_at(FIRST, now).await.0,
            FailoverAttemptDecision::Attempt(_)
        ));
        assert!(matches!(
            controller.begin_attempt_at(RELATED, now).await.0,
            FailoverAttemptDecision::Attempt(_)
        ));
    }

    #[tokio::test]
    async fn stale_failure_does_not_reopen_recovered_circuit() {
        let controller = FailoverController::new(1, 1, Duration::ZERO);
        let now = Instant::now();
        let (FailoverAttemptDecision::Attempt(stale), _) =
            controller.begin_attempt_at(FIRST, now).await
        else {
            panic!("first primary command should be attempted");
        };
        let (FailoverAttemptDecision::Attempt(failing), _) =
            controller.begin_attempt_at(FIRST, now).await
        else {
            panic!("second primary command should be attempted");
        };
        controller
            .record_failure_at(FIRST, failing, FailoverScope::Command, now)
            .await;
        let (FailoverAttemptDecision::Attempt(probe), _) =
            controller.begin_attempt_at(FIRST, now).await
        else {
            panic!("recovery probe should be admitted");
        };
        controller.record_success(FIRST, probe).await;
        assert_eq!(
            controller
                .record_failure_at(FIRST, stale, FailoverScope::Command, now)
                .await,
            [None, None]
        );
    }
}
