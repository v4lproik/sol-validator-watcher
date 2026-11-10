//! Solana JSON-RPC transport, wire models, batching, and capability probes.
//!
//! This module owns protocol decoding and HTTP outcome classification. It does
//! not choose connectors or apply cross-connector fallback policy.

use super::model::{
    AccountBalanceTarget, AccountBalances, ClusterPerformanceMetrics, NodeHealth, NodeRpcStatus,
    RecentBlockFeeRewards, RewardSample,
};
use crate::{
    config::{has_explicit_authority, RpcTransportConfig},
    failover::tag_source_requirement_failure,
    mode::{ProviderFeature, RpcRequestOutcome, SolanaRpcCompatibility},
    telemetry::ConnectorRpcTelemetry,
    transport::build_http_client,
};
use anyhow::{bail, Context, Result};
use reqwest::{Client, StatusCode, Url};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
    time::Instant,
};
use tokio::sync::Mutex;
use tracing::warn;

/// Largest inclusive slot range sent in one `getBlockProduction` request.
pub(super) const MAX_BLOCK_PRODUCTION_RANGE_SLOTS: u64 = 5_000;
const MAX_INFLATION_REWARD_ADDRESSES_PER_REQUEST: usize = 100;
const MAX_ACCOUNT_BALANCE_TARGETS_PER_REQUEST: usize = 50;
/// Maximum number of recent assigned slots requested in one `getBlock` batch.
pub(super) const MAX_RECENT_BLOCK_FEE_REWARD_SLOTS: usize = 32;
const MAX_CACHED_BLOCK_FEE_RESPONSES: usize = MAX_RECENT_BLOCK_FEE_REWARD_SLOTS * 2;

type CachedBlockResponse = Option<Arc<BlockResponse>>;

#[derive(Default)]
struct BlockFeeRewardCache {
    by_slot: BTreeMap<u64, Arc<BlockResponse>>,
}

struct BlockFeeRewardPlan {
    by_slot: HashMap<u64, CachedBlockResponse>,
    missing_slots: Vec<u64>,
}
/// Connector-local Solana JSON-RPC client with shared response telemetry.
#[derive(Clone)]
pub struct RpcClient {
    http: Client,
    url: Url,
    commitment: String,
    response_size_limit_bytes: usize,
    block_fee_reward_cache: Arc<Mutex<BlockFeeRewardCache>>,
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
        let http = build_http_client(
            transport.request_timeout_sec,
            transport.connect_timeout_sec,
            &transport.headers,
        )?;

        Ok(Self {
            http,
            url,
            commitment: transport.commitment_config()?,
            response_size_limit_bytes: transport.response_size_limit_bytes,
            block_fee_reward_cache: Arc::new(Mutex::new(BlockFeeRewardCache::default())),
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
            let (_, epoch_info, (), (), (), (), ()) = tokio::try_join!(
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
                self.probe_required_method::<Vec<ClusterNode>, _>(
                    "getClusterNodes",
                    serde_json::json!([]),
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
            if features.contains(&ProviderFeature::BlockFeeRewards) {
                self.get_block_fee_rewards(&[probe_slot], None)
                    .await
                    .with_context(|| {
                        format!(
                            "RPC endpoint does not satisfy provider feature {:?}",
                            ProviderFeature::BlockFeeRewards.as_str()
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
    pub async fn node_health(&self) -> Result<NodeHealth> {
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
            (Ok(_), Err(error)) => Err(error.into()),
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
    /// Returns public gossip peers and their validator software metadata.
    pub(super) async fn get_cluster_nodes(&self) -> Result<Vec<ClusterNode>> {
        self.call("getClusterNodes", [(); 0]).await
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

    /// Returns identity and vote-account balances plus vote-account rent floors.
    ///
    /// Each `getMultipleAccounts` request contains at most 100 addresses: one
    /// identity and one vote account for each target. The output preserves target
    /// order, and each distinct vote-account size incurs one rent-floor request.
    pub(super) async fn get_account_balances(
        &self,
        targets: &[AccountBalanceTarget<'_>],
    ) -> Result<Vec<Option<AccountBalances>>> {
        if targets.is_empty() {
            return Ok(Vec::new());
        }

        let mut raw_balances = Vec::with_capacity(targets.len());
        for target_chunk in targets.chunks(MAX_ACCOUNT_BALANCE_TARGETS_PER_REQUEST) {
            let mut addresses = Vec::with_capacity(target_chunk.len() * 2);
            for target in target_chunk {
                addresses.push(target.identity);
                addresses.push(target.vote_account);
            }
            let expected_accounts = addresses.len();
            let response: RpcValueResponse<Vec<Option<AccountValue>>> = self
                .call(
                    "getMultipleAccounts",
                    (
                        addresses,
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
            if response.value.len() != expected_accounts {
                self.telemetry.record_failure("getMultipleAccounts")?;
                bail!(
                    "getMultipleAccounts returned {} accounts for {expected_accounts} addresses",
                    response.value.len()
                );
            }

            let mut accounts = response.value.into_iter();
            for _ in target_chunk {
                let identity = accounts
                    .next()
                    .context("getMultipleAccounts response ended before an identity account")?;
                let vote_account = accounts
                    .next()
                    .context("getMultipleAccounts response ended before a vote account")?;
                raw_balances.push(match (identity, vote_account) {
                    (Some(identity), Some(vote_account)) => {
                        Some((identity.lamports, vote_account.lamports, vote_account.space))
                    }
                    _ => None,
                });
            }
        }

        let mut vote_account_spaces = raw_balances
            .iter()
            .filter_map(|balances| balances.as_ref().and_then(|(_, _, space)| *space))
            .collect::<Vec<_>>();
        vote_account_spaces.sort_unstable();
        vote_account_spaces.dedup();
        let mut rent_minimums = HashMap::with_capacity(vote_account_spaces.len());
        for space in vote_account_spaces {
            let minimum = self
                .call(
                    "getMinimumBalanceForRentExemption",
                    (
                        space,
                        CommitmentOnly {
                            commitment: &self.commitment,
                        },
                    ),
                )
                .await?;
            rent_minimums.insert(space, minimum);
        }
        if raw_balances
            .iter()
            .filter_map(Option::as_ref)
            .any(|(_, _, space)| space.is_none())
        {
            warn!(
                "one or more vote account responses omitted account data size; rent and VAT metrics will be unavailable"
            );
        }

        Ok(raw_balances
            .into_iter()
            .map(|balances| {
                balances.map(
                    |(identity_lamports, vote_account_lamports, vote_account_space)| {
                        AccountBalances {
                            identity_lamports,
                            vote_account_lamports,
                            vote_account_rent_exempt_minimum_lamports: vote_account_space
                                .and_then(|space| rent_minimums.get(&space).copied()),
                        }
                    },
                )
            })
            .collect())
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
    /// Returns validator fee rewards from a bounded batch of recent assigned slots.
    ///
    /// Skipped slots are omitted. Finalized produced blocks are cached by slot;
    /// confirmed responses are always fetched because they may be rolled back. A
    /// produced block must contain exactly one non-negative fee reward for
    /// `identity`; malformed rewards fail the command so connector fallback can run.
    pub(super) async fn get_recent_block_fee_rewards(
        &self,
        slots: &[u64],
        identity: &str,
    ) -> Result<Option<RecentBlockFeeRewards>> {
        self.get_block_fee_rewards(slots, Some(identity)).await
    }

    async fn get_block_fee_rewards(
        &self,
        slots: &[u64],
        identity: Option<&str>,
    ) -> Result<Option<RecentBlockFeeRewards>> {
        Self::validate_block_fee_slots(slots)?;
        if slots.is_empty() {
            return Ok(None);
        }

        let cache_enabled = self.block_fee_cache_enabled();
        let plan = self.plan_block_fee_rewards(slots).await;
        let methods_requested = u64::try_from(plan.missing_slots.len())
            .context("block-fee method request count does not fit u64")?;
        let (hits, misses) = if cache_enabled {
            let misses = methods_requested;
            let hits = u64::try_from(slots.len() - plan.missing_slots.len())
                .context("block-fee cache hit count does not fit u64")?;
            (hits, misses)
        } else {
            (0, 0)
        };
        self.telemetry
            .record_block_fee_cache(hits, misses, methods_requested)?;

        let started_at = Instant::now();
        let result = self.resolve_block_fee_rewards(slots, identity, plan).await;
        let outcome = if result.is_ok() {
            RpcRequestOutcome::Succeeded
        } else {
            RpcRequestOutcome::Failed
        };
        let telemetry_result = if methods_requested == 0 {
            if result.is_err() {
                self.telemetry.record_failure("getBlock")
            } else {
                Ok(())
            }
        } else {
            self.telemetry
                .record_request("getBlock", started_at.elapsed(), outcome)
        };

        match (result, telemetry_result) {
            (Ok(result), Ok(())) => Ok(result),
            (Ok(_), Err(error)) => Err(error.into()),
            (Err(error), Ok(())) => Err(error),
            (Err(error), Err(telemetry_error)) => {
                Err(error.context(format!("failed to record RPC request: {telemetry_error:#}")))
            }
        }
    }

    fn validate_block_fee_slots(slots: &[u64]) -> Result<()> {
        if slots.len() > MAX_RECENT_BLOCK_FEE_REWARD_SLOTS {
            bail!(
                "getBlock batch contains {} slots; maximum is {}",
                slots.len(),
                MAX_RECENT_BLOCK_FEE_REWARD_SLOTS
            );
        }
        for (index, slot) in slots.iter().enumerate() {
            if slots[..index].contains(slot) {
                bail!("getBlock batch contains duplicate slot {slot}");
            }
        }
        Ok(())
    }

    fn block_fee_cache_enabled(&self) -> bool {
        self.commitment == "finalized"
    }

    async fn plan_block_fee_rewards(&self, slots: &[u64]) -> BlockFeeRewardPlan {
        if !self.block_fee_cache_enabled() {
            return BlockFeeRewardPlan {
                by_slot: HashMap::with_capacity(slots.len()),
                missing_slots: slots.to_vec(),
            };
        }

        let cache = self.block_fee_reward_cache.lock().await;
        let mut by_slot = HashMap::with_capacity(slots.len());
        let mut missing_slots = Vec::with_capacity(slots.len());
        for slot in slots {
            if let Some(block) = cache.by_slot.get(slot) {
                by_slot.insert(*slot, Some(Arc::clone(block)));
            } else {
                missing_slots.push(*slot);
            }
        }
        BlockFeeRewardPlan {
            by_slot,
            missing_slots,
        }
    }

    async fn resolve_block_fee_rewards(
        &self,
        slots: &[u64],
        identity: Option<&str>,
        mut plan: BlockFeeRewardPlan,
    ) -> Result<Option<RecentBlockFeeRewards>> {
        if !plan.missing_slots.is_empty() {
            let fetched = self.fetch_block_fee_responses(&plan.missing_slots).await?;
            self.cache_block_fee_responses(&fetched).await;
            plan.by_slot.extend(fetched);
        }
        Self::aggregate_block_fee_rewards(slots, identity, &plan.by_slot)
    }

    async fn fetch_block_fee_responses(
        &self,
        slots: &[u64],
    ) -> Result<HashMap<u64, CachedBlockResponse>> {
        let requests = slots
            .iter()
            .map(|slot| {
                JsonRpcRequest::with_id(
                    *slot,
                    "getBlock",
                    (
                        *slot,
                        GetBlockConfig {
                            commitment: &self.commitment,
                            transaction_details: "none",
                            rewards: true,
                        },
                    ),
                )
            })
            .collect::<Vec<_>>();
        let (status, responses): (StatusCode, Vec<JsonRpcEnvelope<BlockResponse>>) =
            self.send_batch_requests("getBlock", &requests).await?;
        if responses.len() != slots.len() {
            bail!(
                "getBlock batch returned {} responses for {} slots",
                responses.len(),
                slots.len()
            );
        }

        let mut seen = [false; MAX_RECENT_BLOCK_FEE_REWARD_SLOTS];
        for response in &responses {
            let Some(index) = slots.iter().position(|slot| *slot == response.id) else {
                bail!(
                    "getBlock batch returned unexpected response id {}",
                    response.id
                );
            };
            if seen[index] {
                bail!(
                    "getBlock batch returned duplicate response id {}",
                    response.id
                );
            }
            seen[index] = true;
            if let Some(error) = &response.error {
                bail!(
                    "getBlock for slot {} failed with JSON-RPC code {}",
                    response.id,
                    error.code
                );
            }
            if matches!(response.result, JsonRpcResult::Missing) {
                bail!(
                    "getBlock for slot {} returned no result or error",
                    response.id
                );
            }
        }
        if !status.is_success() {
            bail!("getBlock returned non-success HTTP status {status}");
        }
        if seen[..slots.len()].contains(&false) {
            bail!("getBlock batch omitted one or more requested slots");
        }

        Ok(responses
            .into_iter()
            .map(|response| {
                let block = match response.result {
                    JsonRpcResult::Value(block) => Some(Arc::new(block)),
                    JsonRpcResult::Null | JsonRpcResult::Missing => None,
                };
                (response.id, block)
            })
            .collect())
    }

    async fn cache_block_fee_responses(&self, fetched: &HashMap<u64, CachedBlockResponse>) {
        if !self.block_fee_cache_enabled() {
            return;
        }
        let mut cache = self.block_fee_reward_cache.lock().await;
        for (slot, block) in fetched {
            if let Some(block) = block {
                cache.by_slot.insert(*slot, Arc::clone(block));
            }
        }
        while cache.by_slot.len() > MAX_CACHED_BLOCK_FEE_RESPONSES {
            let Some((&oldest_slot, _)) = cache.by_slot.first_key_value() else {
                break;
            };
            cache.by_slot.remove(&oldest_slot);
        }
    }

    fn aggregate_block_fee_rewards(
        slots: &[u64],
        identity: Option<&str>,
        responses: &HashMap<u64, CachedBlockResponse>,
    ) -> Result<Option<RecentBlockFeeRewards>> {
        let Some(identity) = identity else {
            return Ok(None);
        };
        let mut latest_slot = None;
        let mut latest_reward_lamports = 0;
        let mut total_reward_lamports = 0_u64;
        let mut sampled_block_count = 0_u64;
        for slot in slots {
            let Some(block) = responses
                .get(slot)
                .with_context(|| format!("getBlock batch omitted requested slot {slot}"))?
            else {
                continue;
            };
            let mut fee_rewards = block.rewards.iter().filter(|reward| {
                reward.reward_type == Some(BlockRewardType::Fee) && reward.pubkey == identity
            });
            let reward = fee_rewards.next().with_context(|| {
                format!("getBlock for slot {slot} returned no fee reward for validator identity")
            })?;
            if fee_rewards.next().is_some() {
                bail!(
                    "getBlock for slot {slot} returned multiple fee rewards for validator identity"
                );
            }
            let reward_lamports = u64::try_from(reward.lamports).with_context(|| {
                format!("getBlock for slot {slot} returned a negative validator fee reward")
            })?;
            total_reward_lamports = total_reward_lamports
                .checked_add(reward_lamports)
                .context("recent block fee reward total overflowed u64")?;
            sampled_block_count += 1;
            if latest_slot.is_none_or(|latest| *slot > latest) {
                latest_slot = Some(*slot);
                latest_reward_lamports = reward_lamports;
            }
        }

        Ok(latest_slot.map(|latest_slot| RecentBlockFeeRewards {
            latest_slot,
            latest_reward_lamports,
            total_reward_lamports,
            sampled_block_count,
        }))
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
            (Ok(_), Err(error)) => Err(error.into()),
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
            .map_err(tag_source_requirement_failure)
            .with_context(|| format!("{method} request failed"))?;
        let (status, body) = self.read_response_body(response, method).await?;
        self.telemetry.record_response(status.as_u16())?;
        let envelope = serde_json::from_slice(&body).with_context(|| {
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
    async fn send_batch_requests<T, P>(
        &self,
        method: &'static str,
        requests: &[JsonRpcRequest<'_, P>],
    ) -> Result<(StatusCode, Vec<JsonRpcEnvelope<T>>)>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        let response = self
            .http
            .post(self.url.clone())
            .json(requests)
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .map_err(tag_source_requirement_failure)
            .with_context(|| format!("{method} batch request failed"))?;
        let (status, body) = self
            .read_response_body(response, &format!("{method} batch"))
            .await?;
        self.telemetry.record_response(status.as_u16())?;
        let envelopes = serde_json::from_slice(&body).with_context(|| {
            if status.is_success() {
                format!("{method} returned an invalid batch JSON-RPC payload")
            } else {
                format!(
                    "{method} returned non-success HTTP status {status} with an invalid batch JSON-RPC payload"
                )
            }
        })?;

        Ok((status, envelopes))
    }
    async fn read_response_body(
        &self,
        mut response: reqwest::Response,
        operation: &str,
    ) -> Result<(StatusCode, Vec<u8>)> {
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > self.response_size_limit_bytes as u64)
        {
            bail!(
                "{operation} response exceeds configured {} byte limit",
                self.response_size_limit_bytes
            );
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
            .with_context(|| format!("{operation} response body read failed"))?
        {
            if chunk.len() > self.response_size_limit_bytes.saturating_sub(body.len()) {
                bail!(
                    "{operation} response exceeds configured {} byte limit",
                    self.response_size_limit_bytes
                );
            }
            body.extend_from_slice(&chunk);
        }

        Ok((status, body))
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
        Self::with_id(1, method, params)
    }

    fn with_id(id: u64, method: &'a str, params: P) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
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
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct GetBlockConfig<'a> {
    commitment: &'a str,
    transaction_details: &'static str,
    rewards: bool,
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
    id: u64,
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

/// Public node entry returned by `getClusterNodes`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClusterNode {
    /// Validator identity account address.
    pub(super) pubkey: String,
    /// Validator client version when advertised.
    pub(super) version: Option<String>,
    /// Feature-set identifier when advertised.
    pub(super) feature_set: Option<u32>,
    /// Shred protocol version when advertised.
    pub(super) shred_version: Option<u16>,
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
#[derive(Deserialize)]
struct BlockResponse {
    rewards: Vec<BlockReward>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlockReward {
    pubkey: String,
    lamports: i64,
    reward_type: Option<BlockRewardType>,
}

#[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
enum BlockRewardType {
    Fee,
    #[serde(other)]
    Other,
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
