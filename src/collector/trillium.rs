//! Bounded Trillium validator-reward API client with conditional revalidation.

use super::{TrilliumBlockRewards, TrilliumMevCapture, TrilliumTransactionMetrics};
use crate::config::{has_explicit_authority, HttpTransportConfig};
use anyhow::{bail, Context, Result};
use reqwest::{
    header::{HeaderValue, IF_MODIFIED_SINCE, LAST_MODIFIED},
    Client, StatusCode, Url,
};
use serde::Deserialize;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const MAX_RESPONSE_BYTES: usize = 1_048_576;
const CACHE_TTL: Duration = Duration::from_secs(60);

/// Client for Trillium's public completed-epoch validator rewards API.
pub(crate) struct TrilliumClient {
    http: Client,
    base_url: Url,
    cache_ttl: Duration,
    cache: Mutex<Option<CachedCapture>>,
}

struct CachedCapture {
    vote_account: String,
    fetched_at: Instant,
    last_modified: Option<HeaderValue>,
    capture: Option<TrilliumMevCapture>,
}

#[derive(Deserialize)]
struct TrilliumRewardRecord {
    epoch: u64,
    vote_account_pubkey: String,
    mev_earned: Option<f64>,
    mev_to_validator: Option<f64>,
    mev_to_stakers: Option<f64>,
    mev_to_jito_block_engine: Option<f64>,
    mev_to_jito_tip_router: Option<f64>,
    avg_mev_per_leader_slot: Option<f64>,
    /// Validator MEV commission in basis points.
    mev_commission: Option<f64>,
    total_jito_tips: Option<f64>,
    total_jito_tip_count: Option<u64>,
    leader_slots: u64,
    blocks_produced: u64,
    total_block_rewards_after_burn: Option<f64>,
    validator_priority_fees: Option<f64>,
    validator_signature_fees: Option<f64>,
    delegator_priority_fees: Option<f64>,
    total_priority_fees: Option<f64>,
    avg_mev_per_block: Option<f64>,
    median_mev_per_block: Option<f64>,
    avg_priority_fees_per_block: Option<f64>,
    median_priority_fees_per_block: Option<f64>,
    avg_signature_fees_per_block: Option<f64>,
    median_signature_fees_per_block: Option<f64>,
    avg_jito_tips_per_block: Option<f64>,
    median_jito_tips_per_block: Option<f64>,
    /// Validator priority-fee commission in basis points.
    priority_fee_commission: Option<f64>,
    activated_stake: Option<f64>,
    avg_tx_per_block: Option<f64>,
    avg_user_tx_per_block: Option<f64>,
    avg_vote_tx_per_block: Option<f64>,
    median_tx_per_block: Option<f64>,
    median_user_tx_per_block: Option<f64>,
    avg_tx_success_rate: Option<f64>,
    avg_user_tx_success_rate: Option<f64>,
    avg_cu_per_block: Option<f64>,
    median_cu_per_block: Option<f64>,
    total_successful_tx: Option<u64>,
    total_failed_tx: Option<u64>,
    total_successful_user_tx: Option<u64>,
    total_failed_user_tx: Option<u64>,
}

#[derive(Deserialize)]
struct TrilliumErrorResponse {
    error: String,
}

impl TrilliumClient {
    /// Builds one reusable HTTP client for a validated Trillium API transport.
    pub(crate) fn try_new(transport: &HttpTransportConfig) -> Result<Self> {
        Self::try_new_with_cache_ttl(transport, CACHE_TTL)
    }

    fn try_new_with_cache_ttl(
        transport: &HttpTransportConfig,
        cache_ttl: Duration,
    ) -> Result<Self> {
        if !has_explicit_authority(&transport.url) {
            bail!("Trillium API URL must use http:// or https:// with a host");
        }
        let base_url = Url::parse(&transport.url).context("invalid Trillium API URL")?;
        if !matches!(base_url.scheme(), "http" | "https") || base_url.host_str().is_none() {
            bail!("Trillium API URL must use http:// or https:// with a host");
        }
        let http = transport
            .build_client()
            .context("failed to build Trillium API HTTP client")?;

        Ok(Self {
            http,
            base_url,
            cache_ttl,
            cache: Mutex::new(None),
        })
    }

    #[cfg(test)]
    pub(crate) fn try_new_for_test(
        transport: &HttpTransportConfig,
        cache_ttl: Duration,
    ) -> Result<Self> {
        Self::try_new_with_cache_ttl(transport, cache_ttl)
    }

    /// Returns the latest completed-epoch MEV capture published for a vote account.
    ///
    /// The public endpoint advertises a 60-second cache lifetime. Revalidation
    /// uses its `Last-Modified` validator so corrected history replaces cached
    /// values without repeatedly downloading the response body.
    pub(crate) async fn get_latest_mev_capture(
        &self,
        vote_account: &str,
    ) -> Result<Option<TrilliumMevCapture>> {
        let mut cache = self.cache.lock().await;
        if let Some(cached) = cache.as_ref().filter(|cached| {
            cached.vote_account == vote_account && cached.fetched_at.elapsed() < self.cache_ttl
        }) {
            return Ok(cached.capture);
        }

        let mut url = self.base_url.clone();
        url.path_segments_mut()
            .map_err(|()| anyhow::anyhow!("Trillium API URL cannot be a base URL"))?
            .pop_if_empty()
            .push("validator_rewards")
            .push(vote_account);
        let matching_cache = cache
            .as_ref()
            .filter(|cached| cached.vote_account == vote_account);
        let mut request = self.http.get(url);
        if let Some(last_modified) = matching_cache.and_then(|cached| cached.last_modified.as_ref())
        {
            request = request.header(IF_MODIFIED_SINCE, last_modified);
        }

        let response = request
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .context("Trillium validator rewards request failed")?;
        let status = response.status();
        if status == StatusCode::NOT_MODIFIED {
            let cached = matching_cache.context(
                "Trillium validator rewards returned HTTP 304 without a cached response",
            )?;
            let capture = cached.capture;
            if let Some(cached) = cache.as_mut() {
                cached.fetched_at = Instant::now();
            }
            return Ok(capture);
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            bail!("Trillium validator rewards response exceeds {MAX_RESPONSE_BYTES} bytes");
        }

        let last_modified = response.headers().get(LAST_MODIFIED).cloned();
        let capacity = response
            .content_length()
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or_default();
        let mut body = Vec::with_capacity(capacity);
        let mut response = response;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(reqwest::Error::without_url)
            .context("Trillium validator rewards response body read failed")?
        {
            if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(body.len()) {
                bail!("Trillium validator rewards response exceeds {MAX_RESPONSE_BYTES} bytes");
            }
            body.extend_from_slice(&chunk);
        }

        if status == StatusCode::NOT_FOUND {
            let is_absent =
                serde_json::from_slice::<TrilliumErrorResponse>(&body).is_ok_and(|response| {
                    response.error == "Validator not found in the last 10 epochs"
                });
            if !is_absent {
                bail!("Trillium validator rewards request returned HTTP {status}");
            }
            *cache = Some(CachedCapture {
                vote_account: vote_account.to_string(),
                fetched_at: Instant::now(),
                last_modified: None,
                capture: None,
            });
            return Ok(None);
        }
        if !status.is_success() {
            bail!("Trillium validator rewards request returned HTTP {status}");
        }

        let records = serde_json::from_slice::<Vec<TrilliumRewardRecord>>(&body)
            .context("Trillium validator rewards response was invalid")?;
        let capture = select_latest_capture(records, vote_account)?;
        *cache = Some(CachedCapture {
            vote_account: vote_account.to_string(),
            fetched_at: Instant::now(),
            last_modified,
            capture,
        });
        Ok(capture)
    }
}

fn select_latest_capture(
    records: Vec<TrilliumRewardRecord>,
    vote_account: &str,
) -> Result<Option<TrilliumMevCapture>> {
    let Some(latest_epoch) = records.iter().map(|record| record.epoch).max() else {
        return Ok(None);
    };
    let mut latest = records
        .into_iter()
        .filter(|record| record.epoch == latest_epoch);
    let record = latest
        .next()
        .context("latest Trillium validator reward record disappeared")?;
    if latest.next().is_some() {
        bail!("Trillium validator rewards response contains duplicate epoch {latest_epoch}");
    }
    if record.vote_account_pubkey != vote_account {
        bail!("Trillium validator rewards response returned an unexpected vote account");
    }
    if record.blocks_produced > record.leader_slots {
        bail!(
            "Trillium validator rewards epoch {} reports more produced blocks than leader slots",
            record.epoch
        );
    }
    validate_nonnegative("mev_earned", record.mev_earned)?;
    validate_nonnegative("mev_to_validator", record.mev_to_validator)?;
    validate_nonnegative("mev_to_stakers", record.mev_to_stakers)?;
    validate_nonnegative("mev_to_jito_block_engine", record.mev_to_jito_block_engine)?;
    validate_nonnegative("mev_to_jito_tip_router", record.mev_to_jito_tip_router)?;
    validate_nonnegative("avg_mev_per_leader_slot", record.avg_mev_per_leader_slot)?;
    validate_nonnegative("total_jito_tips", record.total_jito_tips)?;
    for (field, value) in [
        (
            "total_block_rewards_after_burn",
            record.total_block_rewards_after_burn,
        ),
        ("validator_priority_fees", record.validator_priority_fees),
        ("validator_signature_fees", record.validator_signature_fees),
        ("delegator_priority_fees", record.delegator_priority_fees),
        ("total_priority_fees", record.total_priority_fees),
        ("avg_mev_per_block", record.avg_mev_per_block),
        ("median_mev_per_block", record.median_mev_per_block),
        (
            "avg_priority_fees_per_block",
            record.avg_priority_fees_per_block,
        ),
        (
            "median_priority_fees_per_block",
            record.median_priority_fees_per_block,
        ),
        (
            "avg_signature_fees_per_block",
            record.avg_signature_fees_per_block,
        ),
        (
            "median_signature_fees_per_block",
            record.median_signature_fees_per_block,
        ),
        ("avg_jito_tips_per_block", record.avg_jito_tips_per_block),
        (
            "median_jito_tips_per_block",
            record.median_jito_tips_per_block,
        ),
        ("activated_stake", record.activated_stake),
        ("avg_tx_per_block", record.avg_tx_per_block),
        ("avg_user_tx_per_block", record.avg_user_tx_per_block),
        ("avg_vote_tx_per_block", record.avg_vote_tx_per_block),
        ("median_tx_per_block", record.median_tx_per_block),
        ("median_user_tx_per_block", record.median_user_tx_per_block),
        ("avg_cu_per_block", record.avg_cu_per_block),
        ("median_cu_per_block", record.median_cu_per_block),
    ] {
        validate_nonnegative(field, value)?;
    }
    validate_percentage("avg_tx_success_rate", record.avg_tx_success_rate)?;
    validate_percentage("avg_user_tx_success_rate", record.avg_user_tx_success_rate)?;
    validate_basis_points("priority_fee_commission", record.priority_fee_commission)?;
    validate_basis_points("mev_commission", record.mev_commission)?;

    Ok(Some(TrilliumMevCapture {
        epoch: record.epoch,
        captured_sol: record.mev_earned,
        validator_sol: record.mev_to_validator,
        stakers_sol: record.mev_to_stakers,
        block_engine_sol: record.mev_to_jito_block_engine,
        tip_router_sol: record.mev_to_jito_tip_router,
        per_leader_slot_sol: record.avg_mev_per_leader_slot,
        commission_pct: record
            .mev_commission
            .map(|commission_bps| commission_bps / 100.0),
        total_jito_tips_sol: record.total_jito_tips,
        total_jito_tip_count: record.total_jito_tip_count,
        leader_slots: record.leader_slots,
        blocks_produced: record.blocks_produced,
        activated_stake_sol: record.activated_stake,
        block_rewards: TrilliumBlockRewards {
            after_burn_sol: record.total_block_rewards_after_burn,
            priority_fees_sol: record.total_priority_fees,
            validator_priority_fees_sol: record.validator_priority_fees,
            delegator_priority_fees_sol: record.delegator_priority_fees,
            validator_signature_fees_sol: record.validator_signature_fees,
            average_mev_per_block_sol: record.avg_mev_per_block,
            median_mev_per_block_sol: record.median_mev_per_block,
            average_priority_fees_per_block_sol: record.avg_priority_fees_per_block,
            median_priority_fees_per_block_sol: record.median_priority_fees_per_block,
            average_signature_fees_per_block_sol: record.avg_signature_fees_per_block,
            median_signature_fees_per_block_sol: record.median_signature_fees_per_block,
            average_jito_tips_per_block_sol: record.avg_jito_tips_per_block,
            median_jito_tips_per_block_sol: record.median_jito_tips_per_block,
            priority_fee_commission_pct: record
                .priority_fee_commission
                .map(|commission_bps| commission_bps / 100.0),
        },
        transactions: TrilliumTransactionMetrics {
            successful: record.total_successful_tx,
            failed: record.total_failed_tx,
            successful_user: record.total_successful_user_tx,
            failed_user: record.total_failed_user_tx,
            average_per_block: record.avg_tx_per_block,
            average_user_per_block: record.avg_user_tx_per_block,
            average_vote_per_block: record.avg_vote_tx_per_block,
            median_per_block: record.median_tx_per_block,
            median_user_per_block: record.median_user_tx_per_block,
            average_success_pct: record.avg_tx_success_rate,
            average_user_success_pct: record.avg_user_tx_success_rate,
            average_compute_units_per_block: record.avg_cu_per_block,
            median_compute_units_per_block: record.median_cu_per_block,
        },
    }))
}

fn validate_nonnegative(field: &str, value: Option<f64>) -> Result<()> {
    if value.is_some_and(|value| !value.is_finite() || value < 0.0) {
        bail!("Trillium validator rewards field {field:?} must be a finite nonnegative number");
    }
    Ok(())
}

fn validate_percentage(field: &str, value: Option<f64>) -> Result<()> {
    if value.is_some_and(|value| !value.is_finite() || !(0.0..=100.0).contains(&value)) {
        bail!("Trillium validator rewards field {field:?} must be a percentage from 0 to 100");
    }
    Ok(())
}

fn validate_basis_points(field: &str, value: Option<f64>) -> Result<()> {
    if value.is_some_and(|value| !value.is_finite() || !(0.0..=10_000.0).contains(&value)) {
        bail!("Trillium validator rewards field {field:?} must be basis points from 0 to 10000");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{select_latest_capture, TrilliumRewardRecord};
    use anyhow::Result;

    #[test]
    fn latest_capture_is_selected_by_epoch_not_response_order() -> Result<()> {
        let Some(capture) =
            select_latest_capture(vec![record(8), record(10), record(9)], "vote-1")?
        else {
            anyhow::bail!("latest capture should exist");
        };

        assert_eq!(capture.epoch, 10);
        assert_eq!(capture.commission_pct, Some(10.0));
        assert_eq!(
            capture.block_rewards.priority_fee_commission_pct,
            Some(20.0)
        );
        assert_eq!(capture.transactions.successful, Some(90));
        Ok(())
    }

    #[test]
    fn rejects_mev_commission_above_ten_thousand_basis_points() -> Result<()> {
        let mut invalid = record(10);
        invalid.mev_commission = Some(10_001.0);

        let Err(error) = select_latest_capture(vec![invalid], "vote-1") else {
            anyhow::bail!("invalid commission should fail");
        };

        assert!(error.to_string().contains("mev_commission"));
        Ok(())
    }

    fn record(epoch: u64) -> TrilliumRewardRecord {
        TrilliumRewardRecord {
            epoch,
            vote_account_pubkey: "vote-1".to_string(),
            mev_earned: Some(1.0),
            mev_to_validator: Some(0.1),
            mev_to_stakers: Some(0.8),
            mev_to_jito_block_engine: Some(0.05),
            mev_to_jito_tip_router: Some(0.05),
            avg_mev_per_leader_slot: Some(0.01),
            mev_commission: Some(1_000.0),
            total_jito_tips: Some(1.05),
            total_jito_tip_count: Some(20),
            leader_slots: 100,
            blocks_produced: 98,
            total_block_rewards_after_burn: Some(2.0),
            validator_priority_fees: Some(1.2),
            validator_signature_fees: Some(0.2),
            delegator_priority_fees: Some(0.3),
            total_priority_fees: Some(1.5),
            avg_mev_per_block: Some(0.01),
            median_mev_per_block: Some(0.008),
            avg_priority_fees_per_block: Some(0.015),
            median_priority_fees_per_block: Some(0.012),
            avg_signature_fees_per_block: Some(0.002),
            median_signature_fees_per_block: Some(0.001),
            avg_jito_tips_per_block: Some(0.011),
            median_jito_tips_per_block: Some(0.009),
            priority_fee_commission: Some(2_000.0),
            activated_stake: Some(2_000.0),
            avg_tx_per_block: Some(100.0),
            avg_user_tx_per_block: Some(80.0),
            avg_vote_tx_per_block: Some(20.0),
            median_tx_per_block: Some(90.0),
            median_user_tx_per_block: Some(70.0),
            avg_tx_success_rate: Some(90.0),
            avg_user_tx_success_rate: Some(85.0),
            avg_cu_per_block: Some(10_000_000.0),
            median_cu_per_block: Some(9_000_000.0),
            total_successful_tx: Some(90),
            total_failed_tx: Some(10),
            total_successful_user_tx: Some(68),
            total_failed_user_tx: Some(12),
        }
    }
}
