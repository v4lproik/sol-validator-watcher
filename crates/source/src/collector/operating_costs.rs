//! Bounded per-epoch operating-cost provider client.

use super::OperatingCost;
use crate::{
    config::{has_explicit_authority, HttpTransportConfig},
    failover::tag_source_requirement_failure,
    transport::build_http_client,
};
use anyhow::{bail, Context, Result};
use reqwest::{Client, StatusCode, Url};
use serde::Deserialize;
use std::collections::HashSet;

const MAX_RESPONSE_BYTES: usize = 1_048_576;
const MAX_OPERATING_COSTS: usize = 64;
const MAX_COST_NAME_BYTES: usize = 128;

/// Client for the operator-provided per-epoch operating-cost API.
pub(crate) struct OperatingCostsClient {
    http: Client,
    endpoint: Url,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OperatingCostsResponse {
    epoch: u64,
    costs: Vec<OperatingCost>,
}

impl OperatingCostsClient {
    /// Builds one reusable HTTP client for a validated operating-cost transport.
    pub(crate) fn try_new(transport: &HttpTransportConfig) -> Result<Self> {
        if !has_explicit_authority(&transport.url) {
            bail!("operating-cost API URL must use http:// or https:// with a host");
        }
        let endpoint = Url::parse(&transport.url).context("invalid operating-cost API URL")?;
        if !matches!(endpoint.scheme(), "http" | "https") || endpoint.host_str().is_none() {
            bail!("operating-cost API URL must use http:// or https:// with a host");
        }
        if endpoint.query_pairs().any(|(name, _)| name == "epoch") {
            bail!("operating-cost API URL must not include the request-owned epoch parameter");
        }
        let http = build_http_client(
            transport.request_timeout_sec,
            transport.connect_timeout_sec,
            &transport.headers,
        )
        .context("failed to build operating-cost API HTTP client")?;

        Ok(Self { http, endpoint })
    }

    /// Fetches and validates the complete operating-cost budget for one epoch.
    pub(crate) async fn get_operating_costs(&self, epoch: u64) -> Result<Vec<OperatingCost>> {
        let mut url = self.endpoint.clone();
        url.query_pairs_mut()
            .append_pair("epoch", &epoch.to_string());
        let mut response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .map_err(tag_source_requirement_failure)
            .context("operating-cost request failed")?;
        let status = response.status();
        if status != StatusCode::OK {
            bail!("operating-cost request returned HTTP {status}");
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            bail!("operating-cost response exceeds {MAX_RESPONSE_BYTES} bytes");
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
            .map_err(tag_source_requirement_failure)
            .context("operating-cost response body read failed")?
        {
            if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(body.len()) {
                bail!("operating-cost response exceeds {MAX_RESPONSE_BYTES} bytes");
            }
            body.extend_from_slice(&chunk);
        }

        let response: OperatingCostsResponse =
            serde_json::from_slice(&body).context("operating-cost response was invalid")?;
        validate_response(response, epoch)
    }
}

fn validate_response(
    mut response: OperatingCostsResponse,
    requested_epoch: u64,
) -> Result<Vec<OperatingCost>> {
    if response.epoch != requested_epoch {
        bail!(
            "operating-cost response returned epoch {}, requested {requested_epoch}",
            response.epoch
        );
    }
    if response.costs.is_empty() {
        bail!("operating-cost response must contain at least one cost");
    }
    if response.costs.len() > MAX_OPERATING_COSTS {
        bail!(
            "operating-cost response contains {} costs; maximum is {MAX_OPERATING_COSTS}",
            response.costs.len()
        );
    }

    for (index, cost) in response.costs.iter_mut().enumerate() {
        let trimmed = cost.name.trim();
        if trimmed.is_empty() {
            bail!("operating-cost response costs[{index}].name must not be empty");
        }
        if trimmed.len() > MAX_COST_NAME_BYTES {
            bail!(
                "operating-cost response costs[{index}].name exceeds {MAX_COST_NAME_BYTES} bytes"
            );
        }
        if trimmed.chars().any(char::is_control) {
            bail!(
                "operating-cost response costs[{index}].name must not contain control characters"
            );
        }
        let trim_start = trimmed.as_ptr() as usize - cost.name.as_ptr() as usize;
        let trim_end = trim_start + trimmed.len();
        if trim_end < cost.name.len() {
            cost.name.truncate(trim_end);
        }
        if trim_start > 0 {
            cost.name.drain(..trim_start);
        }
    }

    let mut names = HashSet::with_capacity(response.costs.len());
    let mut total = 0_u64;
    for cost in &response.costs {
        if !names.insert(cost.name.as_str()) {
            bail!(
                "operating-cost response contains duplicate cost name {:?}",
                cost.name
            );
        }
        total = total
            .checked_add(cost.lamports_per_epoch)
            .context("operating-cost response total exceeds the u64 lamport limit")?;
    }

    Ok(response.costs)
}
