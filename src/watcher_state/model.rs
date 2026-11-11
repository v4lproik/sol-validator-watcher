//! Watcher-state transitions owned by the runtime actor.

use crate::mode::WatcherStateSnapshot;

/// A completed poll transition and the state published with it.
#[derive(Debug)]
pub(crate) struct PollCompletion {
    /// Whether this transition newly admitted the watcher as ready.
    pub admitted: bool,
    /// State after applying the completed poll.
    pub state: WatcherStateSnapshot,
}
