//! Deterministic scope-label assignment for validator records.
//!
//! Labels are derived from validated watched-validator configuration and the
//! built-in network and watched scopes.

use crate::config::Config;
use std::collections::{BTreeSet, HashMap};

/// Scope label applied to every observed validator.
pub const LABEL_SCOPE_ALL_NETWORK: &str = "scope:all-network";
/// Scope label applied to validators not explicitly watched.
pub const LABEL_SCOPE_NETWORK: &str = "scope:network";
/// Scope label applied to explicitly watched validators.
pub const LABEL_SCOPE_WATCHED: &str = "scope:watched";

/// Precompiled identity and vote-account label lookup.
#[derive(Clone, Debug, Default)]
pub struct LabelMatcher {
    by_identity: HashMap<String, BTreeSet<String>>,
    by_vote_account: HashMap<String, BTreeSet<String>>,
}

impl LabelMatcher {
    /// Builds deterministic lookup sets from validated watcher configuration.
    pub fn from_config(config: &Config) -> Self {
        let mut matcher = Self::default();

        for watched in &config.watched_validators {
            let user_labels = watched.labels.iter().cloned().collect::<BTreeSet<_>>();

            if let Some(identity) = &watched.identity {
                matcher
                    .by_identity
                    .entry(identity.clone())
                    .or_default()
                    .extend(user_labels.iter().cloned());
            }
            if let Some(vote_account) = &watched.vote_account {
                matcher
                    .by_vote_account
                    .entry(vote_account.clone())
                    .or_default()
                    .extend(user_labels.iter().cloned());
            }
        }

        matcher
    }

    /// Returns sorted, deduplicated built-in and configured labels for a validator.
    pub fn labels_for(&self, identity: &str, vote_account: &str) -> Vec<String> {
        let identity_labels = self.by_identity.get(identity);
        let vote_account_labels = self.by_vote_account.get(vote_account);
        let scope = if identity_labels.is_some() || vote_account_labels.is_some() {
            LABEL_SCOPE_WATCHED
        } else {
            LABEL_SCOPE_NETWORK
        };

        std::iter::once(LABEL_SCOPE_ALL_NETWORK.to_string())
            .chain(
                identity_labels
                    .into_iter()
                    .flat_map(|labels| labels.iter().cloned()),
            )
            .chain(
                vote_account_labels
                    .into_iter()
                    .flat_map(|labels| labels.iter().cloned()),
            )
            .chain(std::iter::once(scope.to_string()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{LabelMatcher, LABEL_SCOPE_ALL_NETWORK, LABEL_SCOPE_NETWORK, LABEL_SCOPE_WATCHED};
    use crate::config::{Config, WatchedValidatorConfig};

    #[test]
    fn resolves_built_in_and_custom_labels() {
        let cfg = Config {
            watched_validators: vec![WatchedValidatorConfig {
                identity: Some("11111111111111111111111111111111".to_string()),
                vote_account: None,
                labels: vec!["region:sbg".to_string(), "vc:validator-1".to_string()],
            }],
            ..Config::default()
        };

        let matcher = LabelMatcher::from_config(&cfg);

        let watched = matcher.labels_for(
            "11111111111111111111111111111111",
            "Vote111111111111111111111111111111111111111",
        );
        assert!(watched.contains(&LABEL_SCOPE_ALL_NETWORK.to_string()));
        assert!(watched.contains(&LABEL_SCOPE_WATCHED.to_string()));
        assert!(watched.contains(&"region:sbg".to_string()));

        let network = matcher.labels_for(
            "SysvarC1ock11111111111111111111111111111111",
            "Vote111111111111111111111111111111111111111",
        );
        assert!(network.contains(&LABEL_SCOPE_ALL_NETWORK.to_string()));
        assert!(network.contains(&LABEL_SCOPE_NETWORK.to_string()));
    }
}
