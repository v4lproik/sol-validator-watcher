//! Bounded Jito validator-reward API client and epoch-stable response cache.

use super::JitoMevReward;
use crate::config::has_explicit_authority;
use anyhow::{bail, Context, Result};
use reqwest::{Client, Url};
use serde::{
    de::{DeserializeSeed, Error as _, SeqAccess, Visitor},
    Deserialize, Deserializer,
};
use std::{fmt, time::Duration, time::Instant};
use tokio::sync::Mutex;

const MAX_RESPONSE_BYTES: usize = 1_048_576;
const ABSENT_REWARD_CACHE_TTL: Duration = Duration::from_secs(60);

/// Client for the provider-specific Jito validator rewards API.
pub(crate) struct JitoClient {
    http: Client,
    base_url: Url,
    cache: Mutex<Option<CachedReward>>,
}

struct CachedReward {
    vote_account: String,
    epoch: u64,
    fetched_at: Instant,
    reward: Option<JitoMevReward>,
}

#[derive(Deserialize)]
struct JitoRewardRecord {
    epoch: u64,
    mev_commission_bps: u16,
    mev_rewards: u64,
}

struct RewardForEpoch {
    epoch: u64,
}

struct RewardForEpochVisitor {
    epoch: u64,
}

impl JitoClient {
    /// Builds one reusable HTTP client for a validated Jito API base URL.
    pub(crate) fn try_new(base_url: &str, timeout: Duration) -> Result<Self> {
        if !has_explicit_authority(base_url) {
            bail!("Jito API URL must use http:// or https:// with a host");
        }
        let base_url = Url::parse(base_url).context("invalid Jito API URL")?;
        if !matches!(base_url.scheme(), "http" | "https") || base_url.host_str().is_none() {
            bail!("Jito API URL must use http:// or https:// with a host");
        }
        let http = Client::builder()
            .timeout(timeout)
            .build()
            .context("failed to build Jito API HTTP client")?;

        Ok(Self {
            http,
            base_url,
            cache: Mutex::new(None),
        })
    }

    /// Returns the selected validator's settled Jito MEV reward for one epoch.
    ///
    /// A settled reward is cached for the life of this source chain. An absent
    /// epoch is retried after the upstream API's documented 60-second cache.
    pub(crate) async fn get_mev_reward(
        &self,
        vote_account: &str,
        epoch: u64,
    ) -> Result<Option<JitoMevReward>> {
        let mut cache = self.cache.lock().await;
        if let Some(cached) = cache.as_ref().filter(|cached| {
            cached.vote_account == vote_account
                && cached.epoch == epoch
                && (cached.reward.is_some()
                    || cached.fetched_at.elapsed() < ABSENT_REWARD_CACHE_TTL)
        }) {
            return Ok(cached.reward);
        }

        let reward = self.fetch_mev_reward(vote_account, epoch).await?;
        *cache = Some(CachedReward {
            vote_account: vote_account.to_string(),
            epoch,
            fetched_at: Instant::now(),
            reward,
        });
        Ok(reward)
    }

    async fn fetch_mev_reward(
        &self,
        vote_account: &str,
        epoch: u64,
    ) -> Result<Option<JitoMevReward>> {
        let mut url = self.base_url.clone();
        let path = format!(
            "{}/validators/{vote_account}",
            self.base_url.path().trim_end_matches('/')
        );
        url.set_path(&path);

        let mut response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .context("Jito validator rewards request failed")?;
        let status = response.status();
        if !status.is_success() {
            bail!("Jito validator rewards request returned HTTP {status}");
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            bail!("Jito validator rewards response exceeds {MAX_RESPONSE_BYTES} bytes");
        }

        let capacity = response
            .content_length()
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or_default();
        let mut body = Vec::with_capacity(capacity);
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(reqwest::Error::without_url)
            .context("Jito validator rewards response body read failed")?
        {
            if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(body.len()) {
                bail!("Jito validator rewards response exceeds {MAX_RESPONSE_BYTES} bytes");
            }
            body.extend_from_slice(&chunk);
        }

        let mut deserializer = serde_json::Deserializer::from_slice(&body);
        let reward = RewardForEpoch { epoch }
            .deserialize(&mut deserializer)
            .context("Jito validator rewards response was invalid")?;
        deserializer
            .end()
            .context("Jito validator rewards response had trailing data")?;
        Ok(reward)
    }
}

impl<'de> DeserializeSeed<'de> for RewardForEpoch {
    type Value = Option<JitoMevReward>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(RewardForEpochVisitor { epoch: self.epoch })
    }
}

impl<'de> Visitor<'de> for RewardForEpochVisitor {
    type Value = Option<JitoMevReward>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an array of Jito validator reward records")
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut selected = None;
        while let Some(record) = sequence.next_element::<JitoRewardRecord>()? {
            if record.epoch != self.epoch {
                continue;
            }
            if record.mev_commission_bps > 10_000 {
                return Err(A::Error::custom(format_args!(
                    "epoch {} has invalid MEV commission {} bps",
                    record.epoch, record.mev_commission_bps
                )));
            }
            let reward = JitoMevReward {
                epoch: record.epoch,
                reward_lamports: record.mev_rewards,
                commission_bps: record.mev_commission_bps,
            };
            if selected.replace(reward).is_some() {
                return Err(A::Error::custom(format_args!(
                    "epoch {} appears more than once",
                    record.epoch
                )));
            }
        }
        Ok(selected)
    }
}
