//! Stable built-in scope labels shared by collection and metrics.

/// Scope label applied to every observed validator.
pub const LABEL_SCOPE_ALL_NETWORK: &str = "scope:all-network";
/// Scope label applied to validators not explicitly watched.
pub const LABEL_SCOPE_NETWORK: &str = "scope:network";
/// Scope label applied to explicitly watched validators.
pub const LABEL_SCOPE_WATCHED: &str = "scope:watched";
