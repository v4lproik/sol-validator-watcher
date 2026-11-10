//! Operator-visible source failover identities and transitions.

use crate::{CommandId, ModeSourceRequirement};
use std::sync::Arc;

/// Runtime scope affected by one connector failure or failover circuit.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FailoverScope {
    /// Every command sharing one connector and exact source requirement.
    SourceRequirement,
    /// Only the command whose provider interaction failed.
    Command,
}

impl FailoverScope {
    /// Returns the stable Prometheus and diagnostic label.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SourceRequirement => "source-requirement",
            Self::Command => "command",
        }
    }
}

/// Operator-visible state of one configured failover circuit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailoverStatus {
    /// Primary routing is active.
    Primary,
    /// Primary routing is cooling down after a failure.
    Open,
    /// One primary recovery probe is in flight.
    RecoveryProbe,
}

impl FailoverStatus {
    /// Returns the stable Prometheus and diagnostic label.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Open => "open",
            Self::RecoveryProbe => "recovery-probe",
        }
    }

    /// Reports whether this circuit is routing away from normal primary use.
    pub const fn is_degraded(self) -> bool {
        !matches!(self, Self::Primary)
    }

    /// Numeric Prometheus value: primary `0`, open `1`, recovery probe `2`.
    pub const fn metric_value(self) -> i64 {
        match self {
            Self::Primary => 0,
            Self::Open => 1,
            Self::RecoveryProbe => 2,
        }
    }
}

/// Stable identity of one source failover circuit.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FailoverKey {
    /// Configured connector name.
    pub connector: Arc<str>,
    /// Operator-declared provider name.
    pub provider: Arc<str>,
    /// Exact source requirement guarded by the circuit.
    pub source_requirement: ModeSourceRequirement,
    /// Shared source-requirement or command-local scope.
    pub scope: FailoverScope,
    /// Command identity for a command-local circuit.
    pub command: Option<CommandId>,
}

/// Reason one failover circuit changed state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailoverTransitionKind {
    /// A failure opened the circuit.
    Activated,
    /// Cooldown elapsed and a recovery probe was admitted.
    RecoveryProbeStarted,
    /// The admitted recovery probe failed.
    RecoveryProbeFailed,
    /// The admitted recovery probe succeeded.
    Recovered,
}

impl FailoverTransitionKind {
    /// Returns the stable diagnostic label.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Activated => "activated",
            Self::RecoveryProbeStarted => "recovery-probe-started",
            Self::RecoveryProbeFailed => "recovery-probe-failed",
            Self::Recovered => "recovered",
        }
    }
}

/// One committed failover circuit transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailoverTransition {
    /// Circuit identity.
    pub key: FailoverKey,
    /// Circuit state after the transition.
    pub status: FailoverStatus,
    /// Reason for the transition.
    pub kind: FailoverTransitionKind,
}
