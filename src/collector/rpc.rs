//! Solana JSON-RPC transport, wire models, batching, and capability probes.
//!
//! This module owns protocol decoding and HTTP outcome classification. It does
//! not choose connectors or apply cross-connector fallback policy.

use super::model::{
    AccountBalances, ClusterPerformanceMetrics, NodeHealth, NodeRpcStatus, RewardSample,
};
use crate::{
    config::{has_explicit_authority, RpcTransportConfig},
    mode::{ProviderFeature, SolanaRpcCompatibility},
    watcher_state::{ConnectorRpcTelemetry, RpcRequestOutcome},
};
use anyhow::{bail, Context, Result};
use reqwest::{Client, StatusCode, Url};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::{collections::HashMap, time::Instant};
use tracing::warn;

/// Largest inclusive slot range sent in one `getBlockProduction` request.
pub(super) const MAX_BLOCK_PRODUCTION_RANGE_SLOTS: u64 = 5_000;
const MAX_INFLATION_REWARD_ADDRESSES_PER_REQUEST: usize = 100;
/// Connector-local Solana JSON-RPC client with shared response telemetry.
#[derive(Clone)]
pub struct RpcClient {
    http: Client,
    url: Url,
    commitment: String,
    telemetry: ConnectorRpcTelemetry,
}

impl RpcClient {
    /// Builds a client for one validated RPC transport.
    pub fn try_new(
        transport: &RpcTransportConfig,
        telemetry: ConnectorRpcTelemetry,
    ) -> Result<Self> {
        if !has_explicit_authority(&transport.url) {
            bail!("Solana RPC URL must use http:// or https:// with a host");
        }
        let url = Url::parse(&transport.url).context("invalid Solana RPC URL")?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            bail!("Solana RPC URL must use http:// or https:// with a host");
        }
        let http = transport.build_client()?;

        Ok(Self {
            http,
            url,
            commitment: transport.commitment_config()?,
            telemetry,
        })
    }

    /// Probes every standard RPC method and provider feature the connector declares.
    ///
    /// Probes use bounded, valid requests and fail before collection if the endpoint
    /// cannot satisfy its declared contract.
    pub async fn verify_capabilities(
        &self,
        solana_rpc_compatibility: SolanaRpcCompatibility,
        features: &[ProviderFeature],
    ) -> Result<()> {
        const PROBE_PUBKEY: &str = "11111111111111111111111111111111";
        let commitment = self.commitment.as_str();

        let epoch_info = async {
            let (_, epoch_info, (), _, _, _) = tokio::try_join!(
                async {
                    self.node_health()
                        .await
                        .context("required RPC method getHealth probe failed")
                },
                async {
                    self.get_epoch_info()
                        .await
                        .context("required RPC method getEpochInfo probe failed")
                },
                self.probe_required_method::<VoteAccounts, _>(
                    "getVoteAccounts",
                    serde_json::json!([{
                        "commitment": commitment,
                        "votePubkey": PROBE_PUBKEY
                    }]),
                ),
                self.probe_required_method::<IdentityResponse, _>(
                    "getIdentity",
                    serde_json::json!([]),
                ),
                self.probe_required_method::<VersionResponse, _>(
                    "getVersion",
                    serde_json::json!([]),
                ),
                self.probe_required_method::<u64, _>(
                    "getBlockHeight",
                    serde_json::json!([{ "commitment": commitment }]),
                ),
            )?;
            self.get_leader_schedule(epoch_info.absolute_slot)
                .await
                .context("required RPC method getLeaderSchedule probe failed")?;
            Ok::<_, anyhow::Error>(epoch_info)
        }
        .await
        .with_context(|| {
            format!(
                "RPC endpoint does not satisfy solana_rpc_compatibility = {:?}",
                SolanaRpcCompatibility::Core.as_str()
            )
        })?;

        if solana_rpc_compatibility == SolanaRpcCompatibility::Extended {
            let probe_slot = epoch_info.absolute_slot.saturating_sub(1);
            tokio::try_join!(
                self.probe_required_method::<Vec<RecentPerformanceSample>, _>(
                    "getRecentPerformanceSamples",
                    serde_json::json!([1]),
                ),
                self.probe_required_method::<RpcValueResponse<BlockProduction>, _>(
                    "getBlockProduction",
                    serde_json::json!([{
                        "commitment": commitment,
                        "range": {
                            "firstSlot": probe_slot,
                            "lastSlot": probe_slot
                        }
                    }]),
                ),
                self.probe_required_method::<RpcValueResponse<Vec<Option<AccountValue>>>, _>(
                    "getMultipleAccounts",
                    serde_json::json!([
                        [PROBE_PUBKEY],
                        {
                            "commitment": commitment,
                            "dataSlice": {
                                "offset": 0,
                                "length": 0
                            },
                            "encoding": "base64"
                        }
                    ]),
                ),
                self.probe_required_method::<u64, _>(
                    "getMinimumBalanceForRentExemption",
                    serde_json::json!([0, { "commitment": commitment }]),
                ),
                self.probe_required_method::<Vec<Option<InflationRewardValue>>, _>(
                    "getInflationReward",
                    serde_json::json!([
                        [PROBE_PUBKEY],
                        {
                            "commitment": commitment
                        }
                    ]),
                ),
                self.probe_required_method::<u64, _>("minimumLedgerSlot", serde_json::json!([]),),
            )
            .with_context(|| {
                let feature_context = features
                    .contains(&ProviderFeature::HistoricalData)
                    .then_some(" required by provider feature \"historical-data\"");
                format!(
                    "RPC endpoint does not satisfy solana_rpc_compatibility = {:?}{}",
                    SolanaRpcCompatibility::Extended.as_str(),
                    feature_context.unwrap_or_default()
                )
            })?;
            if let (true, Some(reward_epoch)) = (
                features.contains(&ProviderFeature::HistoricalData),
                epoch_info.epoch.checked_sub(1),
            ) {
                self.probe_required_method::<Vec<Option<InflationRewardValue>>, _>(
                    "getInflationReward",
                    serde_json::json!([
                        [PROBE_PUBKEY],
                        {
                            "commitment": commitment,
                            "epoch": reward_epoch
                        }
                    ]),
                )
                .await
                .with_context(|| {
                    format!(
                        "RPC endpoint does not satisfy provider feature {:?}",
                        ProviderFeature::HistoricalData.as_str()
                    )
                })?;
            }
        }

        Ok(())
    }

    async fn probe_required_method<T, P>(&self, method: &'static str, params: P) -> Result<()>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        self.call::<T, _>(method, params)
            .await
            .with_context(|| format!("required RPC method {method} probe failed"))?;
        Ok(())
    }

    /// Returns current epoch and absolute-slot information.
    pub(super) async fn get_epoch_info(&self) -> Result<EpochInfo> {
        self.call(
            "getEpochInfo",
            [CommitmentOnly {
                commitment: &self.commitment,
            }],
        )
        .await
    }

    /// Returns node health while preserving Solana's structured behind state.
    pub(crate) async fn node_health(&self) -> Result<NodeHealth> {
        let started_at = Instant::now();
        let result = self.node_health_request().await;
        let outcome = match result {
            Ok(NodeHealth::Healthy) => RpcRequestOutcome::Succeeded,
            Ok(NodeHealth::Behind { .. }) | Err(_) => RpcRequestOutcome::Failed,
        };
        let telemetry_result =
            self.telemetry
                .record_request("getHealth", started_at.elapsed(), outcome);

        match (result, telemetry_result) {
            (Ok(result), Ok(())) => Ok(result),
            (Ok(_), Err(error)) => Err(error),
            (Err(error), Ok(())) => Err(error),
            (Err(error), Err(telemetry_error)) => {
                Err(error.context(format!("failed to record RPC request: {telemetry_error:#}")))
            }
        }
    }

    async fn node_health_request(&self) -> Result<NodeHealth> {
        let response: JsonRpcResponse<String> = self.send_request("getHealth", [(); 0]).await?;
        let status = response.status;
        let envelope = response.envelope;

        if let Some(error) = envelope.error {
            if error.code == -32005 {
                let slots = error
                    .data
                    .as_ref()
                    .and_then(|data| data.get("numSlotsBehind"))
                    .and_then(Value::as_u64);
                return Ok(NodeHealth::Behind { slots });
            }
            bail!("getHealth failed with JSON-RPC code {}", error.code);
        }
        if !status.is_success() {
            bail!("getHealth returned non-success HTTP status {status}");
        }

        match envelope.result {
            JsonRpcResult::Value(result) if result == "ok" => Ok(NodeHealth::Healthy),
            JsonRpcResult::Value(result) => {
                bail!("RPC endpoint returned unexpected getHealth result {result:?}")
            }
            JsonRpcResult::Null => bail!("getHealth returned no result"),
            JsonRpcResult::Missing => bail!("getHealth returned no result or error"),
        }
    }

    /// Collects coherent identity, software, block-height, and health values.
    pub(super) async fn node_status(&self) -> Result<NodeRpcStatus> {
        let (health, identity, version, block_height) = tokio::try_join!(
            self.node_health(),
            self.call::<IdentityResponse, _>("getIdentity", [(); 0]),
            self.call::<VersionResponse, _>("getVersion", [(); 0]),
            self.call::<u64, _>(
                "getBlockHeight",
                [CommitmentOnly {
                    commitment: &self.commitment,
                }],
            ),
        )?;

        Ok(NodeRpcStatus {
            identity: identity.identity,
            version: version.solana_core,
            feature_set: version.feature_set,
            block_height,
            health,
        })
    }

    /// Returns current and delinquent vote accounts.
    pub(super) async fn get_vote_accounts(
        &self,
        delinquent_slot_distance: Option<u64>,
    ) -> Result<VoteAccounts> {
        self.call(
            "getVoteAccounts",
            [GetVoteAccountsConfig {
                commitment: &self.commitment,
                keep_unstaked_delinquents: true,
                delinquent_slot_distance,
            }],
        )
        .await
    }

    /// Derives cluster throughput and slot timing from the latest performance sample.
    ///
    /// Returns `None` when the endpoint has no sample. A zero-duration sample or
    /// an impossible transaction split is rejected so fallback can try another
    /// configured connector.
    pub(super) async fn get_cluster_performance_metrics(
        &self,
    ) -> Result<Option<ClusterPerformanceMetrics>> {
        let samples: Vec<RecentPerformanceSample> =
            self.call("getRecentPerformanceSamples", [1]).await?;
        let Some(sample) = samples.first() else {
            return Ok(None);
        };
        if sample.sample_period_secs == 0 {
            bail!("getRecentPerformanceSamples returned a zero-duration sample");
        }

        let sample_period_seconds = f64::from(sample.sample_period_secs);
        let (non_vote_transactions_per_second, vote_transactions_per_second) = match sample
            .num_non_vote_transactions
        {
            Some(non_vote_transactions) => {
                let vote_transactions = sample
                        .num_transactions
                        .checked_sub(non_vote_transactions)
                        .context(
                            "getRecentPerformanceSamples returned more non-vote than total transactions",
                        )?;
                (
                    Some(non_vote_transactions as f64 / sample_period_seconds),
                    Some(vote_transactions as f64 / sample_period_seconds),
                )
            }
            None => (None, None),
        };

        Ok(Some(ClusterPerformanceMetrics {
            average_slot_time_seconds: (sample.num_slots > 0)
                .then(|| sample_period_seconds / sample.num_slots as f64),
            transactions_per_second: sample.num_transactions as f64 / sample_period_seconds,
            vote_transactions_per_second,
            non_vote_transactions_per_second,
            slots_per_second: sample.num_slots as f64 / sample_period_seconds,
        }))
    }

    /// Returns block production for the endpoint's current default range.
    pub(super) async fn get_current_epoch_block_production(&self) -> Result<BlockProduction> {
        let response: RpcValueResponse<BlockProduction> = self
            .call(
                "getBlockProduction",
                [GetBlockProductionConfig {
                    commitment: &self.commitment,
                    range: None,
                }],
            )
            .await?;

        Ok(response.value)
    }

    /// Returns merged block production for an inclusive slot range.
    ///
    /// Large ranges are split into bounded sequential requests.
    pub(super) async fn get_block_production_for_range(
        &self,
        first_slot: u64,
        last_slot: u64,
    ) -> Result<BlockProduction> {
        if last_slot < first_slot {
            bail!(
                "getBlockProduction requires last_slot >= first_slot, got {} < {}",
                last_slot,
                first_slot
            );
        }

        let mut block_production = BlockProduction::default();
        let mut next_first_slot = first_slot;

        loop {
            let next_last_slot = last_slot.min(
                next_first_slot.saturating_add(MAX_BLOCK_PRODUCTION_RANGE_SLOTS.saturating_sub(1)),
            );
            let response: RpcValueResponse<BlockProduction> = self
                .call(
                    "getBlockProduction",
                    [GetBlockProductionConfig {
                        commitment: &self.commitment,
                        range: Some(SlotRange {
                            first_slot: next_first_slot,
                            last_slot: next_last_slot,
                        }),
                    }],
                )
                .await?;

            merge_block_production(&mut block_production, response.value);

            if next_last_slot == last_slot {
                break;
            }

            next_first_slot = next_last_slot.saturating_add(1);
        }

        Ok(block_production)
    }

    /// Returns the leader schedule containing `slot`, if retained by the endpoint.
    pub(super) async fn get_leader_schedule(
        &self,
        slot: u64,
    ) -> Result<Option<HashMap<String, Vec<u64>>>> {
        self.call_optional(
            "getLeaderSchedule",
            (
                slot,
                CommitmentOnly {
                    commitment: &self.commitment,
                },
            ),
        )
        .await
    }

    /// Returns the earliest slot retained by the endpoint.
    pub(super) async fn get_minimum_ledger_slot(&self) -> Result<u64> {
        self.call("minimumLedgerSlot", [(); 0]).await
    }

    /// Returns identity and vote-account balances plus the vote account's rent floor.
    pub(super) async fn get_account_balances(
        &self,
        identity: &str,
        vote_account: &str,
    ) -> Result<AccountBalances> {
        let response: RpcValueResponse<Vec<Option<AccountValue>>> = self
            .call(
                "getMultipleAccounts",
                (
                    [identity, vote_account],
                    GetMultipleAccountsConfig {
                        commitment: &self.commitment,
                        encoding: "base64",
                        data_slice: DataSlice {
                            offset: 0,
                            length: 0,
                        },
                    },
                ),
            )
            .await?;
        if response.value.len() != 2 {
            self.telemetry.record_failure("getMultipleAccounts")?;
            bail!(
                "getMultipleAccounts returned {} accounts for 2 addresses",
                response.value.len()
            );
        }
        let mut accounts = response.value.into_iter();
        let identity = accounts
            .next()
            .flatten()
            .context("identity account was not found")?;
        let vote_account = accounts
            .next()
            .flatten()
            .context("vote account was not found")?;
        let rent_exempt_minimum_lamports = match vote_account.space {
            Some(space) => Some(
                self.call(
                    "getMinimumBalanceForRentExemption",
                    (
                        space,
                        CommitmentOnly {
                            commitment: &self.commitment,
                        },
                    ),
                )
                .await?,
            ),
            None => {
                warn!(
                    "vote account response omitted account data size; rent and VAT metrics will be unavailable"
                );
                None
            }
        };

        Ok(AccountBalances {
            identity_lamports: identity.lamports,
            vote_account_lamports: vote_account.lamports,
            vote_account_rent_exempt_minimum_lamports: rent_exempt_minimum_lamports,
        })
    }

    /// Returns available inflation rewards keyed by requested address.
    ///
    /// Requests are batched to the endpoint limit. Missing or negative rewards are
    /// omitted rather than represented as zero.
    pub(super) async fn get_inflation_rewards(
        &self,
        addresses: &[String],
        epoch: u64,
    ) -> Result<HashMap<String, RewardSample>> {
        if addresses.is_empty() {
            return Ok(HashMap::new());
        }

        let mut by_address = HashMap::with_capacity(addresses.len());
        for addresses_chunk in addresses.chunks(MAX_INFLATION_REWARD_ADDRESSES_PER_REQUEST) {
            let rewards: Vec<Option<InflationRewardValue>> = self
                .call(
                    "getInflationReward",
                    (
                        addresses_chunk,
                        GetInflationRewardConfig {
                            commitment: &self.commitment,
                            epoch,
                        },
                    ),
                )
                .await?;

            if rewards.len() != addresses_chunk.len() {
                self.telemetry.record_failure("getInflationReward")?;
                bail!(
                    "getInflationReward returned {} rewards for {} addresses",
                    rewards.len(),
                    addresses_chunk.len()
                );
            }

            for (address, reward) in addresses_chunk.iter().zip(rewards) {
                let Some(reward) = reward else {
                    continue;
                };
                let Ok(amount_lamports) = u64::try_from(reward.amount) else {
                    continue;
                };

                by_address.insert(
                    address.clone(),
                    RewardSample {
                        epoch: reward.epoch,
                        amount_lamports,
                    },
                );
            }
        }

        Ok(by_address)
    }

    async fn call<T, P>(&self, method: &'static str, params: P) -> Result<T>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        match self.call_optional(method, params).await? {
            Some(result) => Ok(result),
            None => {
                self.telemetry.record_failure(method)?;
                bail!("{method} returned no result");
            }
        }
    }

    /// Executes one JSON-RPC request whose valid result may be JSON `null`.
    ///
    /// Transport, HTTP, payload, and JSON-RPC failures increment failed-attempt
    /// telemetry before being returned.
    pub(super) async fn call_optional<T, P>(
        &self,
        method: &'static str,
        params: P,
    ) -> Result<Option<T>>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        let started_at = Instant::now();
        let result = self.call_optional_request(method, params).await;
        let outcome = if result.is_ok() {
            RpcRequestOutcome::Succeeded
        } else {
            RpcRequestOutcome::Failed
        };
        let telemetry_result = self
            .telemetry
            .record_request(method, started_at.elapsed(), outcome);

        match (result, telemetry_result) {
            (Ok(result), Ok(())) => Ok(result),
            (Ok(_), Err(error)) => Err(error),
            (Err(error), Ok(())) => Err(error),
            (Err(error), Err(telemetry_error)) => {
                Err(error.context(format!("failed to record RPC request: {telemetry_error:#}")))
            }
        }
    }

    async fn call_optional_request<T, P>(
        &self,
        method: &'static str,
        params: P,
    ) -> Result<Option<T>>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        let response = self.send_request(method, params).await?;
        let status = response.status;
        let envelope = response.envelope;

        if let Some(error) = envelope.error {
            bail!("{method} failed with JSON-RPC code {}", error.code);
        }
        if !status.is_success() {
            bail!("{} returned non-success HTTP status {}", method, status);
        }

        match envelope.result {
            JsonRpcResult::Missing => bail!("{method} returned no result or error"),
            JsonRpcResult::Null => Ok(None),
            JsonRpcResult::Value(result) => Ok(Some(result)),
        }
    }

    async fn send_request<T, P>(
        &self,
        method: &'static str,
        params: P,
    ) -> Result<JsonRpcResponse<T>>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        let response = self
            .http
            .post(self.url.clone())
            .json(&JsonRpcRequest::new(method, params))
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .with_context(|| format!("{method} request failed"))?;
        let status = response.status();
        self.telemetry.record_response(status.as_u16())?;
        let body = response
            .text()
            .await
            .map_err(reqwest::Error::without_url)
            .with_context(|| format!("{method} response body read failed"))?;
        let envelope = serde_json::from_str(&body).with_context(|| {
            if status.is_success() {
                format!("{} returned an invalid JSON-RPC payload", method)
            } else {
                format!(
                    "{} returned non-success HTTP status {} with an invalid JSON-RPC payload",
                    method, status
                )
            }
        })?;

        Ok(JsonRpcResponse { status, envelope })
    }
}
fn merge_block_production(target: &mut BlockProduction, update: BlockProduction) {
    for (identity, (leader_slots, blocks_produced)) in update.by_identity {
        let entry = target.by_identity.entry(identity).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(leader_slots);
        entry.1 = entry.1.saturating_add(blocks_produced);
    }
}
#[derive(Serialize)]
struct JsonRpcRequest<'a, P> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: P,
}

impl<'a, P> JsonRpcRequest<'a, P> {
    fn new(method: &'a str, params: P) -> Self {
        Self {
            jsonrpc: "2.0",
            id: 1,
            method,
            params,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CommitmentOnly<'a> {
    commitment: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GetVoteAccountsConfig<'a> {
    commitment: &'a str,
    keep_unstaked_delinquents: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    delinquent_slot_distance: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GetMultipleAccountsConfig<'a> {
    commitment: &'a str,
    encoding: &'static str,
    data_slice: DataSlice,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DataSlice {
    offset: usize,
    length: usize,
}

/// Wire configuration for `getBlockProduction`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GetBlockProductionConfig<'a> {
    /// Requested commitment.
    pub(super) commitment: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Optional inclusive slot range.
    pub(super) range: Option<SlotRange>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GetInflationRewardConfig<'a> {
    commitment: &'a str,
    epoch: u64,
}

/// Inclusive absolute slot range.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SlotRange {
    /// First requested slot.
    pub(super) first_slot: u64,
    /// Last requested slot.
    pub(super) last_slot: u64,
}

struct JsonRpcResponse<T> {
    status: StatusCode,
    envelope: JsonRpcEnvelope<T>,
}

#[derive(Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
struct JsonRpcEnvelope<T> {
    #[serde(default, deserialize_with = "deserialize_json_rpc_result")]
    result: JsonRpcResult<T>,
    error: Option<JsonRpcError>,
}

#[derive(Default)]
enum JsonRpcResult<T> {
    #[default]
    Missing,
    Null,
    Value(T),
}

fn deserialize_json_rpc_result<'de, D, T>(
    deserializer: D,
) -> std::result::Result<JsonRpcResult<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(match Option::<T>::deserialize(deserializer)? {
        Some(result) => JsonRpcResult::Value(result),
        None => JsonRpcResult::Null,
    })
}

#[derive(Deserialize)]
struct JsonRpcError {
    code: i64,
    data: Option<Value>,
}

#[derive(Deserialize)]
struct IdentityResponse {
    identity: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct VersionResponse {
    solana_core: String,
    feature_set: Option<u32>,
}

/// Standard Solana RPC response wrapper containing a `value`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RpcValueResponse<T> {
    /// Decoded response value.
    pub(super) value: T,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecentPerformanceSample {
    num_transactions: u64,
    num_non_vote_transactions: Option<u64>,
    num_slots: u64,
    sample_period_secs: u16,
}

#[derive(Deserialize)]
struct AccountValue {
    lamports: u64,
    space: Option<u64>,
}

/// Decoded fields used from `getEpochInfo`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EpochInfo {
    /// Current epoch.
    pub(super) epoch: u64,
    /// Current zero-based slot offset within the epoch.
    pub(super) slot_index: u64,
    /// Total slots in the epoch.
    pub(super) slots_in_epoch: u64,
    /// Current absolute slot.
    pub(super) absolute_slot: u64,
}

/// Decoded current and delinquent lists from `getVoteAccounts`.
#[derive(Deserialize)]
pub(crate) struct VoteAccounts {
    /// Vote accounts participating in the current set.
    pub(super) current: Vec<VoteAccountInfo>,
    /// Vote accounts classified as delinquent.
    pub(super) delinquent: Vec<VoteAccountInfo>,
}

/// Decoded fields used from one vote-account record.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VoteAccountInfo {
    /// Vote-account address.
    pub(super) vote_pubkey: String,
    /// Validator identity address.
    pub(super) node_pubkey: String,
    /// Activated stake in lamports.
    pub(super) activated_stake: u64,
    /// Commission percentage.
    pub(super) commission: u8,
    /// Tuples of epoch, cumulative credits, and prior cumulative credits.
    pub(super) epoch_credits: Vec<(u64, u64, u64)>,
    /// Most recent voted slot.
    pub(super) last_vote: u64,
    /// Most recent rooted slot.
    pub(super) root_slot: u64,
}

/// Decoded `getBlockProduction` values keyed by validator identity.
#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BlockProduction {
    /// Assigned leader slots and produced blocks for each identity.
    pub(super) by_identity: HashMap<String, (u64, u64)>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InflationRewardValue {
    amount: i64,
    epoch: u64,
}
