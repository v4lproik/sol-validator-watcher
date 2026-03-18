use crate::config::Config;
use std::collections::{BTreeSet, HashMap};

pub const LABEL_SCOPE_ALL_NETWORK: &str = "scope:all-network";
pub const LABEL_SCOPE_NETWORK: &str = "scope:network";
pub const LABEL_SCOPE_WATCHED: &str = "scope:watched";

#[derive(Clone, Debug, Default)]
pub struct LabelMatcher {
    by_identity: HashMap<String, BTreeSet<String>>,
    by_vote_account: HashMap<String, BTreeSet<String>>,
}

impl LabelMatcher {
    pub fn from_config(config: &Config) -> Self {
        let mut matcher = Self::default();

        for watched in &config.watched_validators {
            let mut user_labels = BTreeSet::new();
            for label in &watched.labels {
                user_labels.insert(label.clone());
            }

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

    pub fn labels_for(&self, identity: &str, vote_account: &str) -> Vec<String> {
        let mut labels = BTreeSet::from([LABEL_SCOPE_ALL_NETWORK.to_string()]);
        let mut watched = false;

        if let Some(extra_labels) = self.by_identity.get(identity) {
            watched = true;
            labels.extend(extra_labels.iter().cloned());
        }

        if let Some(extra_labels) = self.by_vote_account.get(vote_account) {
            watched = true;
            labels.extend(extra_labels.iter().cloned());
        }

        if watched {
            labels.insert(LABEL_SCOPE_WATCHED.to_string());
        } else {
            labels.insert(LABEL_SCOPE_NETWORK.to_string());
        }

        labels.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{LabelMatcher, LABEL_SCOPE_ALL_NETWORK, LABEL_SCOPE_NETWORK, LABEL_SCOPE_WATCHED};
    use crate::config::{Config, WatchedValidatorConfig};

    #[test]
    fn resolves_built_in_and_custom_labels() {
        let cfg = Config {
            network: "mainnet-beta".to_string(),
            rpc_url: "http://127.0.0.1:8899".to_string(),
            rpc_timeout_sec: 30,
            metrics_port: 8000,
            poll_interval_sec: 15,
            commitment: "confirmed".to_string(),
            delinquent_slot_distance: None,
            liveness_file: None,
            watched_validators: vec![WatchedValidatorConfig {
                identity: Some("11111111111111111111111111111111".to_string()),
                vote_account: None,
                labels: vec!["region:sbg".to_string(), "vc:validator-1".to_string()],
            }],
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
