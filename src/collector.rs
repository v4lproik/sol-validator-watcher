pub mod command;

use crate::{
    config::Config,
    labels::{LabelMatcher, LABEL_SCOPE_ALL_NETWORK, LABEL_SCOPE_NETWORK, LABEL_SCOPE_WATCHED},
    mode::{CommandId, ModeId, ProviderFeature, SolanaRpcCompatibility, Transport},
    source::{SourceChain, SourceSelection},
};
use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tracing::warn;

const MAX_BLOCK_PRODUCTION_RANGE_SLOTS: u64 = 5_000;
const MAX_INFLATION_REWARD_ADDRESSES_PER_REQUEST: usize = 100;

const VAT_LAMPORTS_PER_EPOCH: u64 = 800_000_000;

#[derive(Clone, Default)]
pub(crate) struct RpcResponseStats {
    inner: Arc<RpcResponseStatsInner>,
}

#[derive(Default)]
struct RpcResponseStatsInner {
    total: AtomicU64,
    successful_2xx: AtomicU64,
    redirection_3xx: AtomicU64,
    client_error_4xx: AtomicU64,
    server_error_5xx: AtomicU64,
    other: AtomicU64,
    status_200: AtomicU64,
    status_403: AtomicU64,
    status_429: AtomicU64,
    status_500: AtomicU64,
    failed_attempts: AtomicU64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RpcResponseSnapshot {
    pub total: u64,
    pub successful_2xx: u64,
    pub redirection_3xx: u64,
    pub client_error_4xx: u64,
    pub server_error_5xx: u64,
    pub other: u64,
    pub status_200: u64,
    pub status_403: u64,
    pub status_429: u64,
    pub status_500: u64,
    pub failed_attempts: u64,
}

impl RpcResponseStats {
    pub fn snapshot(&self) -> RpcResponseSnapshot {
        RpcResponseSnapshot {
            total: self.inner.total.load(Ordering::Relaxed),
            successful_2xx: self.inner.successful_2xx.load(Ordering::Relaxed),
            redirection_3xx: self.inner.redirection_3xx.load(Ordering::Relaxed),
            client_error_4xx: self.inner.client_error_4xx.load(Ordering::Relaxed),
            server_error_5xx: self.inner.server_error_5xx.load(Ordering::Relaxed),
            other: self.inner.other.load(Ordering::Relaxed),
            status_200: self.inner.status_200.load(Ordering::Relaxed),
            status_403: self.inner.status_403.load(Ordering::Relaxed),
            status_429: self.inner.status_429.load(Ordering::Relaxed),
            status_500: self.inner.status_500.load(Ordering::Relaxed),
            failed_attempts: self.inner.failed_attempts.load(Ordering::Relaxed),
        }
    }

    fn record(&self, status: reqwest::StatusCode) {
        self.inner.total.fetch_add(1, Ordering::Relaxed);
        if status.is_success() {
            self.inner.successful_2xx.fetch_add(1, Ordering::Relaxed);
        } else if status.is_redirection() {
            self.inner.redirection_3xx.fetch_add(1, Ordering::Relaxed);
        } else if status.is_client_error() {
            self.inner.client_error_4xx.fetch_add(1, Ordering::Relaxed);
        } else if status.is_server_error() {
            self.inner.server_error_5xx.fetch_add(1, Ordering::Relaxed);
        } else {
            self.inner.other.fetch_add(1, Ordering::Relaxed);
        }

        match status.as_u16() {
            200 => &self.inner.status_200,
            403 => &self.inner.status_403,
            429 => &self.inner.status_429,
            500 => &self.inner.status_500,
            _ => return,
        }
        .fetch_add(1, Ordering::Relaxed);
    }

    fn record_failure(&self) {
        self.inner.failed_attempts.fetch_add(1, Ordering::Relaxed);
    }
}

impl RpcResponseSnapshot {
    pub fn delta_since(self, previous: Self) -> Self {
        Self {
            total: self.total.saturating_sub(previous.total),
            successful_2xx: self.successful_2xx.saturating_sub(previous.successful_2xx),
            redirection_3xx: self
                .redirection_3xx
                .saturating_sub(previous.redirection_3xx),
            client_error_4xx: self
                .client_error_4xx
                .saturating_sub(previous.client_error_4xx),
            server_error_5xx: self
                .server_error_5xx
                .saturating_sub(previous.server_error_5xx),
            other: self.other.saturating_sub(previous.other),
            status_200: self.status_200.saturating_sub(previous.status_200),
            status_403: self.status_403.saturating_sub(previous.status_403),
            status_429: self.status_429.saturating_sub(previous.status_429),
            status_500: self.status_500.saturating_sub(previous.status_500),
            failed_attempts: self
                .failed_attempts
                .saturating_sub(previous.failed_attempts),
        }
    }
    pub fn non_success_count(self) -> u64 {
        self.total.saturating_sub(self.successful_2xx)
    }
}

#[derive(Clone)]
pub struct RpcClient {
    http: Client,
    url: String,
    commitment: String,
    responses: RpcResponseStats,
}

impl RpcClient {
    pub fn new(
        url: String,
        timeout: Duration,
        commitment: String,
        responses: RpcResponseStats,
    ) -> Result<Self> {
        let http = Client::builder()
            .timeout(timeout)
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self {
            http,
            url,
            commitment,
            responses,
        })
    }

    pub async fn verify_capabilities(
        &self,
        solana_rpc_compatibility: SolanaRpcCompatibility,
        features: &[ProviderFeature],
    ) -> Result<()> {
        const PROBE_PUBKEY: &str = "11111111111111111111111111111111";
        let commitment = self.commitment.as_str();

        let epoch_info = async {
            let ((), epoch_info, ()) = tokio::try_join!(
                self.ensure_healthy(),
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

    async fn get_epoch_info(&self) -> Result<EpochInfo> {
        self.call(
            "getEpochInfo",
            [CommitmentOnly {
                commitment: &self.commitment,
            }],
        )
        .await
    }

    async fn get_health(&self) -> Result<String> {
        self.call("getHealth", [(); 0]).await
    }

    async fn get_vote_accounts(
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

    async fn get_average_slot_time_seconds(&self) -> Result<Option<f64>> {
        let samples: Vec<RecentPerformanceSample> =
            self.call("getRecentPerformanceSamples", [1]).await?;
        let Some(sample) = samples.first() else {
            return Ok(None);
        };
        if sample.num_slots == 0 || sample.sample_period_secs == 0 {
            return Ok(None);
        }

        Ok(Some(
            f64::from(sample.sample_period_secs) / sample.num_slots as f64,
        ))
    }

    async fn get_current_epoch_block_production(&self) -> Result<BlockProduction> {
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

    async fn get_block_production_for_range(
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

    async fn get_leader_schedule(&self, slot: u64) -> Result<Option<HashMap<String, Vec<u64>>>> {
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

    async fn get_minimum_ledger_slot(&self) -> Result<u64> {
        self.call("minimumLedgerSlot", [(); 0]).await
    }

    async fn get_account_balances(
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
            self.responses.record_failure();
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

    async fn get_inflation_rewards(
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
                self.responses.record_failure();
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

    pub(crate) async fn ensure_healthy(&self) -> Result<()> {
        let health = self
            .get_health()
            .await
            .context("RPC endpoint failed getHealth")?;

        if health != "ok" {
            self.responses.record_failure();
            bail!("RPC endpoint returned unexpected getHealth result {health:?}");
        }

        Ok(())
    }

    async fn call<T, P>(&self, method: &'static str, params: P) -> Result<T>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        match self.call_optional(method, params).await? {
            Some(result) => Ok(result),
            None => {
                self.responses.record_failure();
                bail!("{method} returned no result");
            }
        }
    }

    async fn call_optional<T, P>(&self, method: &'static str, params: P) -> Result<Option<T>>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        let result = self.call_optional_request(method, params).await;
        if result.is_err() {
            self.responses.record_failure();
        }
        result
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
        let response = self
            .http
            .post(&self.url)
            .json(&JsonRpcRequest::new(method, params))
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .with_context(|| format!("{method} request failed"))?;
        let status = response.status();
        self.responses.record(status);
        let body = response
            .text()
            .await
            .map_err(reqwest::Error::without_url)
            .with_context(|| format!("{method} response body read failed"))?;

        let envelope: JsonRpcEnvelope<T> = serde_json::from_str(&body).with_context(|| {
            if status.is_success() {
                format!("{} returned an invalid JSON-RPC payload", method)
            } else {
                format!(
                    "{} returned non-success HTTP status {} with an invalid JSON-RPC payload",
                    method, status
                )
            }
        })?;

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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidatorStatus {
    Current,
    Delinquent,
}

impl ValidatorStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Delinquent => "delinquent",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EpochCreditsSample {
    pub epoch: u64,
    pub earned: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RewardSample {
    pub epoch: u64,
    pub amount_lamports: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AccountBalances {
    pub identity_lamports: u64,
    pub vote_account_lamports: u64,
    pub vote_account_rent_exempt_minimum_lamports: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EconomicsMetrics {
    pub identity: String,
    pub vote_account: String,
    pub identity_balance_lamports: u64,
    pub vote_account_balance_lamports: u64,
    pub vote_account_rent_exempt_minimum_lamports: Option<u64>,
    pub vat_reserve_lamports: Option<u64>,
    pub vat_epochs_available: Option<u64>,
    pub vat_next_epoch_funded: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WatchedValidatorMetrics {
    pub identity: String,
    pub vote_account: String,
    pub status: ValidatorStatus,
    pub scopes: Vec<String>,
    pub activated_stake_lamports: u64,
    pub latest_epoch_vote_credits: Option<u64>,
    pub vote_efficiency_pct: Option<f64>,
    pub last_vote_slot_lag: u64,
    pub root_slot_lag: u64,
    pub commission_pct: u8,
    pub latest_epoch_reward_lamports: u64,
    pub latest_epoch_rewarded_blocks_produced: u64,
    pub blocks_produced: u64,
    pub blocks_missed: u64,
    pub block_production_rate: f64,
    pub future_leader_slots: u64,
    pub next_leader_slot: Option<u64>,
    pub slots_until_next_leader: Option<u64>,
    pub seconds_until_next_leader: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValidatorRecord {
    pub identity: String,
    pub vote_account: String,
    pub status: ValidatorStatus,
    pub activated_stake_lamports: u64,
    pub commission: u8,
    pub last_vote: u64,
    pub root_slot: u64,
    pub latest_epoch_credits: Option<EpochCreditsSample>,
    pub latest_epoch_reward: Option<RewardSample>,
    pub latest_epoch_rewarded_blocks_produced: u64,
    pub current_epoch_leader_slots: u64,
    pub current_epoch_blocks_produced: u64,
    pub future_leader_slots: u64,
    pub next_leader_slot: Option<u64>,
    pub slots_until_next_leader: Option<u64>,
    pub seconds_until_next_leader: Option<f64>,
    pub scopes: Vec<String>,
}

impl ValidatorRecord {
    fn is_watched(&self) -> bool {
        self.scopes.iter().any(|scope| scope == LABEL_SCOPE_WATCHED)
    }

    fn last_vote_slot_lag(&self, current_slot: u64) -> u64 {
        current_slot.saturating_sub(self.last_vote)
    }

    fn root_slot_lag(&self, current_slot: u64) -> u64 {
        current_slot.saturating_sub(self.root_slot)
    }

    fn matching_epoch_credits(&self, sample_epoch: Option<u64>) -> Option<u64> {
        self.latest_epoch_credits
            .as_ref()
            .filter(|credits| Some(credits.epoch) == sample_epoch)
            .map(|credits| credits.earned)
    }

    fn matching_epoch_reward_lamports(&self, reward_sample_epoch: Option<u64>) -> Option<u64> {
        self.latest_epoch_reward
            .as_ref()
            .filter(|reward| Some(reward.epoch) == reward_sample_epoch)
            .map(|reward| reward.amount_lamports)
    }

    fn vote_efficiency_pct(
        &self,
        ideal_epoch_credits: Option<u64>,
        sample_epoch: Option<u64>,
    ) -> Option<f64> {
        let credits = self.matching_epoch_credits(sample_epoch)?;
        let ideal = ideal_epoch_credits.filter(|ideal| *ideal > 0)?;

        Some(ratio(credits, ideal))
    }

    fn blocks_missed(&self) -> u64 {
        self.current_epoch_leader_slots
            .saturating_sub(self.current_epoch_blocks_produced)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScopeMetrics {
    pub current_count: u64,
    pub delinquent_count: u64,
    pub current_stake_lamports: u64,
    pub delinquent_stake_lamports: u64,
    pub latest_epoch_vote_credits: Option<u64>,
    pub latest_epoch_reward_lamports: u64,
    pub latest_epoch_rewarded_blocks_produced: u64,
    pub vote_efficiency_pct: Option<f64>,
    pub last_vote_slot_lag: f64,
    pub root_slot_lag: f64,
    pub average_commission_pct: f64,
    pub blocks_produced: u64,
    pub blocks_missed: u64,
    pub block_production_rate: f64,
    pub future_leader_slots: u64,
}

impl ScopeMetrics {
    pub(crate) fn activated_stake_lamports(&self) -> u64 {
        self.current_stake_lamports + self.delinquent_stake_lamports
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub network: String,
    pub epoch: u64,
    pub slot: u64,
    pub epoch_progress_pct: f64,
    pub epoch_slots_remaining: u64,
    pub average_slot_time_seconds: Option<f64>,
    pub estimated_epoch_end_unixtime: Option<f64>,
    pub vote_credits_sample_epoch: Option<u64>,
    pub reward_sample_epoch: Option<u64>,
    pub scopes: BTreeMap<String, ScopeMetrics>,
    pub watched_validators: Vec<WatchedValidatorMetrics>,
    pub data_sources: Vec<SnapshotDataSource>,
    pub mode_statuses: Vec<ModeStatus>,
    pub economics: Option<EconomicsMetrics>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModeStatus {
    pub mode: ModeId,
    pub up: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotDataSource {
    pub mode: ModeId,
    pub command: CommandId,
    pub connector: Arc<str>,
    pub provider: Arc<str>,
    pub transport: Transport,
    pub solana_rpc_compatibility: Option<SolanaRpcCompatibility>,
    pub feature: Option<ProviderFeature>,
    pub precedence: usize,
}

impl From<SourceSelection> for SnapshotDataSource {
    fn from(source: SourceSelection) -> Self {
        Self {
            mode: source.mode,
            command: source.command,
            connector: source.connector,
            provider: source.provider,
            transport: source.transport,
            solana_rpc_compatibility: source.solana_rpc_compatibility,
            feature: source.feature,
            precedence: source.precedence,
        }
    }
}

#[derive(Default)]
struct ScopeAccumulator {
    current_count: u64,
    delinquent_count: u64,
    current_stake_lamports: u64,
    delinquent_stake_lamports: u64,
    latest_epoch_vote_credits: Option<u64>,
    latest_epoch_reward_lamports: u64,
    latest_epoch_rewarded_blocks_produced: u64,
    weighted_vote_efficiency_sum: f64,
    vote_efficiency_weight_sum: f64,
    weighted_last_vote_slot_lag_sum: f64,
    weighted_root_slot_lag_sum: f64,
    weighted_commission_sum: f64,
    weight_sum: f64,
    blocks_produced: u64,
    blocks_missed: u64,
    future_leader_slots: u64,
}

impl ScopeAccumulator {
    fn observe(
        &mut self,
        record: &ValidatorRecord,
        current_slot: u64,
        ideal_epoch_credits: Option<u64>,
        sample_epoch: Option<u64>,
        reward_sample_epoch: Option<u64>,
    ) {
        match record.status {
            ValidatorStatus::Current => {
                self.current_count += 1;
                self.current_stake_lamports += record.activated_stake_lamports;
            }
            ValidatorStatus::Delinquent => {
                self.delinquent_count += 1;
                self.delinquent_stake_lamports += record.activated_stake_lamports;
            }
        }

        let weight = record.activated_stake_lamports.max(1) as f64;
        self.weight_sum += weight;
        self.weighted_last_vote_slot_lag_sum +=
            weight * record.last_vote_slot_lag(current_slot) as f64;
        self.weighted_root_slot_lag_sum += weight * record.root_slot_lag(current_slot) as f64;
        self.weighted_commission_sum += weight * f64::from(record.commission);

        if let Some(credits) = record.matching_epoch_credits(sample_epoch) {
            self.latest_epoch_vote_credits = Some(
                self.latest_epoch_vote_credits
                    .unwrap_or_default()
                    .saturating_add(credits),
            );
        }
        if let Some(vote_efficiency_pct) =
            record.vote_efficiency_pct(ideal_epoch_credits, sample_epoch)
        {
            self.weighted_vote_efficiency_sum += weight * vote_efficiency_pct;
            self.vote_efficiency_weight_sum += weight;
        }

        if let Some(reward_lamports) = record.matching_epoch_reward_lamports(reward_sample_epoch) {
            self.latest_epoch_reward_lamports += reward_lamports;
            self.latest_epoch_rewarded_blocks_produced +=
                record.latest_epoch_rewarded_blocks_produced;
        }

        self.blocks_produced += record.current_epoch_blocks_produced;
        self.blocks_missed += record.blocks_missed();
        self.future_leader_slots += record.future_leader_slots;
    }

    fn finish(self) -> ScopeMetrics {
        let weight_sum = if self.weight_sum > 0.0 {
            self.weight_sum
        } else {
            1.0
        };
        let total_blocks = self.blocks_produced + self.blocks_missed;

        ScopeMetrics {
            current_count: self.current_count,
            delinquent_count: self.delinquent_count,
            current_stake_lamports: self.current_stake_lamports,
            delinquent_stake_lamports: self.delinquent_stake_lamports,
            latest_epoch_vote_credits: self.latest_epoch_vote_credits,
            latest_epoch_reward_lamports: self.latest_epoch_reward_lamports,
            latest_epoch_rewarded_blocks_produced: self.latest_epoch_rewarded_blocks_produced,
            vote_efficiency_pct: (self.vote_efficiency_weight_sum > 0.0)
                .then(|| self.weighted_vote_efficiency_sum / self.vote_efficiency_weight_sum),
            last_vote_slot_lag: self.weighted_last_vote_slot_lag_sum / weight_sum,
            root_slot_lag: self.weighted_root_slot_lag_sum / weight_sum,
            average_commission_pct: self.weighted_commission_sum / weight_sum,
            blocks_produced: self.blocks_produced,
            blocks_missed: self.blocks_missed,
            block_production_rate: ratio(self.blocks_produced, total_blocks),
            future_leader_slots: self.future_leader_slots,
        }
    }
}

pub async fn ensure_economics_supported(sources: &SourceChain, config: &Config) -> Result<()> {
    if !config.modes.economics.enabled {
        return Ok(());
    }

    let epoch_command = command::GetEpochInfo;
    let vote_accounts_command = command::GetVoteAccounts {
        delinquent_slot_distance: config.delinquent_slot_distance,
    };
    let (epoch_info, vote_accounts) = tokio::join!(
        sources.required(&epoch_command),
        sources.required(&vote_accounts_command),
    );
    let epoch_info = epoch_info?;
    let vote_accounts = vote_accounts?;
    let target = resolve_economics_target(config, &vote_accounts.value)?;
    sources
        .required(&command::GetAccountBalances {
            identity: &target.identity,
            vote_account: &target.vote_account,
        })
        .await?;

    let Some(reward_epoch) = epoch_info.value.epoch.checked_sub(1) else {
        return Ok(());
    };
    let current_slot = epoch_info.value.absolute_slot;
    let epoch_start_slot = current_slot.saturating_sub(epoch_info.value.slot_index);
    sources
        .required(&command::GetInflationRewards {
            addresses: std::slice::from_ref(&target.vote_account),
            epoch: reward_epoch,
        })
        .await?;
    sources
        .required(&command::GetRewardEpochBlockProduction {
            epoch_start_slot,
            slots_in_epoch: epoch_info.value.slots_in_epoch,
        })
        .await?;

    Ok(())
}

pub async fn collect_snapshot(sources: &SourceChain, config: &Config) -> Result<Snapshot> {
    let epoch_command = command::GetEpochInfo;
    let vote_accounts_command = command::GetVoteAccounts {
        delinquent_slot_distance: config.delinquent_slot_distance,
    };
    let (epoch_info, vote_accounts, recent_performance) = tokio::join!(
        sources.required(&epoch_command),
        sources.required(&vote_accounts_command),
        sources.optional(&command::GetRecentPerformanceSamples),
    );
    let epoch_info = epoch_info?;
    let vote_accounts = vote_accounts?;
    let mut data_sources = vec![
        epoch_info.source.clone().into(),
        vote_accounts.source.clone().into(),
    ];
    let average_slot_time_seconds = recent_performance.and_then(|resolved| {
        data_sources.push(resolved.source.into());
        resolved.value
    });

    let current_slot = epoch_info.value.absolute_slot;
    let epoch_start_slot = current_slot.saturating_sub(epoch_info.value.slot_index);
    let next_epoch_start_slot = epoch_start_slot.saturating_add(epoch_info.value.slots_in_epoch);
    let reward_epoch = epoch_info.value.epoch.checked_sub(1);
    let economics_enabled = config.modes.economics.enabled;
    let economics_target = economics_enabled
        .then(|| resolve_economics_target(config, &vote_accounts.value))
        .transpose()?;
    let reward_vote_accounts = economics_enabled.then(|| {
        vote_accounts
            .value
            .current
            .iter()
            .chain(vote_accounts.value.delinquent.iter())
            .map(|info| info.vote_pubkey.clone())
            .collect::<Vec<_>>()
    });
    let current_schedule_command = command::GetLeaderSchedule { slot: current_slot };
    let next_schedule_command = command::GetNextLeaderSchedule {
        slot: next_epoch_start_slot,
    };

    let (current_block_production, current_schedule, next_schedule, economics_outputs) = tokio::join!(
        sources.optional(&command::GetCurrentBlockProduction),
        sources.optional(&current_schedule_command),
        sources.optional(&next_schedule_command),
        async {
            let Some(target) = economics_target.as_ref() else {
                return Ok::<_, anyhow::Error>((None, None, None));
            };
            let balances = sources
                .required(&command::GetAccountBalances {
                    identity: &target.identity,
                    vote_account: &target.vote_account,
                })
                .await?;

            let (Some(epoch), Some(addresses)) = (reward_epoch, reward_vote_accounts.as_deref())
            else {
                return Ok((Some(balances), None, None));
            };
            let inflation_rewards_command = command::GetInflationRewards { addresses, epoch };
            let block_production_command = command::GetRewardEpochBlockProduction {
                epoch_start_slot,
                slots_in_epoch: epoch_info.value.slots_in_epoch,
            };
            let (inflation_rewards, block_production) = tokio::join!(
                sources.required(&inflation_rewards_command),
                sources.required(&block_production_command),
            );
            Ok((
                Some(balances),
                Some(inflation_rewards?),
                Some(block_production?),
            ))
        },
    );

    let (account_balances, inflation_rewards, reward_epoch_block_production) = economics_outputs?;
    let leadership_up =
        current_block_production.is_some() && current_schedule.is_some() && next_schedule.is_some();

    let block_production = match current_block_production {
        Some(resolved) => {
            data_sources.push(resolved.source.into());
            resolved.value
        }
        None => BlockProduction::default(),
    };
    let current_schedule = current_schedule.and_then(|resolved| {
        data_sources.push(resolved.source.into());
        resolved.value
    });
    let next_schedule = next_schedule.and_then(|resolved| {
        data_sources.push(resolved.source.into());
        resolved.value
    });
    let economics = match (economics_target.as_ref(), account_balances) {
        (Some(target), Some(resolved)) => {
            data_sources.push(resolved.source.into());
            Some(build_economics_metrics(target, resolved.value))
        }
        _ => None,
    };
    let inflation_rewards = inflation_rewards
        .map(|resolved| {
            data_sources.push(resolved.source.into());
            resolved.value
        })
        .unwrap_or_default();
    let reward_epoch_block_production = reward_epoch_block_production
        .map(|resolved| {
            data_sources.push(resolved.source.into());
            resolved.value
        })
        .unwrap_or_default();

    let matcher = LabelMatcher::from_config(config);
    let build_context = RecordBuildContext {
        matcher: &matcher,
        current_slot,
        current_epoch: epoch_info.value.epoch,
        epoch_start_slot,
        next_epoch_start_slot,
        average_slot_time_seconds,
        by_identity: &block_production.by_identity,
        reward_epoch_by_identity: &reward_epoch_block_production.by_identity,
        current_schedule: current_schedule.as_ref(),
        next_schedule: next_schedule.as_ref(),
        latest_epoch_rewards: &inflation_rewards,
    };
    let mut records = Vec::with_capacity(
        vote_accounts.value.current.len() + vote_accounts.value.delinquent.len(),
    );

    for info in vote_accounts.value.current {
        records.push(build_record(info, ValidatorStatus::Current, &build_context));
    }
    for info in vote_accounts.value.delinquent {
        records.push(build_record(
            info,
            ValidatorStatus::Delinquent,
            &build_context,
        ));
    }

    let mut snapshot = aggregate_records(
        config.network.clone(),
        epoch_info.value.epoch,
        current_slot,
        &records,
    );
    snapshot.epoch_progress_pct =
        ratio(epoch_info.value.slot_index, epoch_info.value.slots_in_epoch);
    snapshot.epoch_slots_remaining = epoch_info
        .value
        .slots_in_epoch
        .saturating_sub(epoch_info.value.slot_index);
    snapshot.average_slot_time_seconds = average_slot_time_seconds;
    snapshot.estimated_epoch_end_unixtime =
        estimated_epoch_end_unixtime(snapshot.epoch_slots_remaining, average_slot_time_seconds);
    snapshot.data_sources = data_sources;
    snapshot.economics = economics;
    snapshot.mode_statuses = vec![
        ModeStatus {
            mode: ModeId::Consensus,
            up: true,
        },
        ModeStatus {
            mode: ModeId::Leadership,
            up: leadership_up,
        },
    ];
    if economics_enabled {
        snapshot.mode_statuses.push(ModeStatus {
            mode: ModeId::Economics,
            up: true,
        });
    }
    Ok(snapshot)
}

struct RecordBuildContext<'a> {
    matcher: &'a LabelMatcher,
    current_epoch: u64,
    current_slot: u64,
    epoch_start_slot: u64,
    next_epoch_start_slot: u64,
    average_slot_time_seconds: Option<f64>,
    by_identity: &'a HashMap<String, (u64, u64)>,
    reward_epoch_by_identity: &'a HashMap<String, (u64, u64)>,
    current_schedule: Option<&'a HashMap<String, Vec<u64>>>,
    next_schedule: Option<&'a HashMap<String, Vec<u64>>>,
    latest_epoch_rewards: &'a HashMap<String, RewardSample>,
}

fn build_record(
    info: VoteAccountInfo,
    status: ValidatorStatus,
    context: &RecordBuildContext<'_>,
) -> ValidatorRecord {
    let (leader_slots, blocks_produced) = context
        .by_identity
        .get(&info.node_pubkey)
        .copied()
        .unwrap_or((0, 0));
    let reward_epoch_blocks_produced = context
        .reward_epoch_by_identity
        .get(&info.node_pubkey)
        .copied()
        .map_or(0, |(_, produced)| produced);

    let current_epoch_future_slots = context
        .current_schedule
        .and_then(|schedule| schedule.get(&info.node_pubkey))
        .into_iter()
        .flatten()
        .filter_map(|relative_slot| context.epoch_start_slot.checked_add(*relative_slot))
        .filter(|absolute_slot| *absolute_slot >= context.current_slot);
    let next_epoch_future_slots = context
        .next_schedule
        .and_then(|schedule| schedule.get(&info.node_pubkey))
        .into_iter()
        .flatten()
        .filter_map(|relative_slot| context.next_epoch_start_slot.checked_add(*relative_slot));
    let (future_leader_slots, next_leader_slot) = current_epoch_future_slots
        .chain(next_epoch_future_slots)
        .fold((0_u64, None), |(count, next), slot| {
            (
                count.saturating_add(1),
                Some(next.map_or(slot, |current: u64| current.min(slot))),
            )
        });
    let slots_until_next_leader =
        next_leader_slot.map(|slot| slot.saturating_sub(context.current_slot));
    let seconds_until_next_leader = slots_until_next_leader
        .zip(context.average_slot_time_seconds)
        .map(|(slots, slot_time)| slots as f64 * slot_time);

    let latest_epoch_credits = info
        .epoch_credits
        .iter()
        .filter(|(epoch, _, _)| *epoch <= context.current_epoch)
        .max_by_key(|(epoch, _, _)| *epoch)
        .map(|(epoch, credits, previous)| EpochCreditsSample {
            epoch: *epoch,
            earned: credits.saturating_sub(*previous),
        });
    let latest_epoch_reward = context.latest_epoch_rewards.get(&info.vote_pubkey).cloned();

    let scopes = context
        .matcher
        .labels_for(&info.node_pubkey, &info.vote_pubkey);

    ValidatorRecord {
        identity: info.node_pubkey,
        vote_account: info.vote_pubkey,
        status,
        activated_stake_lamports: info.activated_stake,
        commission: info.commission,
        last_vote: info.last_vote,
        root_slot: info.root_slot,
        latest_epoch_credits,
        latest_epoch_reward,
        latest_epoch_rewarded_blocks_produced: reward_epoch_blocks_produced,
        current_epoch_leader_slots: leader_slots,
        current_epoch_blocks_produced: blocks_produced,
        future_leader_slots,
        next_leader_slot,
        slots_until_next_leader,
        seconds_until_next_leader,
        scopes,
    }
}

pub fn aggregate_records(
    network: String,
    epoch: u64,
    slot: u64,
    records: &[ValidatorRecord],
) -> Snapshot {
    let sample_epoch = records
        .iter()
        .filter_map(|record| {
            record
                .latest_epoch_credits
                .as_ref()
                .map(|credits| credits.epoch)
        })
        .filter(|sample_epoch| *sample_epoch <= epoch)
        .max();
    let reward_sample_epoch = records
        .iter()
        .filter_map(|record| {
            record
                .latest_epoch_reward
                .as_ref()
                .map(|reward| reward.epoch)
        })
        .max();

    let ideal_epoch_credits = sample_epoch.and_then(|expected_epoch| {
        records
            .iter()
            .filter_map(|record| record.latest_epoch_credits.as_ref())
            .filter(|credits| credits.epoch == expected_epoch)
            .map(|credits| credits.earned)
            .max()
    });

    let mut scopes: BTreeMap<String, ScopeAccumulator> = BTreeMap::new();
    scopes
        .entry(LABEL_SCOPE_ALL_NETWORK.to_string())
        .or_default();
    scopes.entry(LABEL_SCOPE_NETWORK.to_string()).or_default();
    scopes.entry(LABEL_SCOPE_WATCHED.to_string()).or_default();

    for record in records {
        for scope in &record.scopes {
            scopes.entry(scope.clone()).or_default().observe(
                record,
                slot,
                ideal_epoch_credits,
                sample_epoch,
                reward_sample_epoch,
            );
        }
    }

    let mut watched_validators = records
        .iter()
        .filter(|record| record.is_watched())
        .map(|record| {
            build_watched_validator_metrics(
                record,
                slot,
                ideal_epoch_credits,
                sample_epoch,
                reward_sample_epoch,
            )
        })
        .collect::<Vec<_>>();
    watched_validators.sort_by(|left, right| {
        left.identity
            .cmp(&right.identity)
            .then_with(|| left.vote_account.cmp(&right.vote_account))
    });

    Snapshot {
        network,
        epoch,
        slot,
        epoch_progress_pct: 0.0,
        epoch_slots_remaining: 0,
        average_slot_time_seconds: None,
        estimated_epoch_end_unixtime: None,
        vote_credits_sample_epoch: sample_epoch,
        reward_sample_epoch,
        scopes: scopes
            .into_iter()
            .map(|(scope, metrics)| (scope, metrics.finish()))
            .collect(),
        watched_validators,
        data_sources: vec![],
        mode_statuses: vec![],
        economics: None,
    }
}

fn build_watched_validator_metrics(
    record: &ValidatorRecord,
    current_slot: u64,
    ideal_epoch_credits: Option<u64>,
    sample_epoch: Option<u64>,
    reward_sample_epoch: Option<u64>,
) -> WatchedValidatorMetrics {
    let latest_epoch_vote_credits = record.matching_epoch_credits(sample_epoch);
    let latest_epoch_reward_lamports = record
        .matching_epoch_reward_lamports(reward_sample_epoch)
        .unwrap_or_default();
    let vote_efficiency_pct = record.vote_efficiency_pct(ideal_epoch_credits, sample_epoch);

    WatchedValidatorMetrics {
        identity: record.identity.clone(),
        vote_account: record.vote_account.clone(),
        status: record.status,
        scopes: record.scopes.clone(),
        activated_stake_lamports: record.activated_stake_lamports,
        latest_epoch_vote_credits,
        vote_efficiency_pct,
        last_vote_slot_lag: record.last_vote_slot_lag(current_slot),
        root_slot_lag: record.root_slot_lag(current_slot),
        commission_pct: record.commission,
        latest_epoch_reward_lamports,
        latest_epoch_rewarded_blocks_produced: if latest_epoch_reward_lamports > 0 {
            record.latest_epoch_rewarded_blocks_produced
        } else {
            0
        },
        blocks_produced: record.current_epoch_blocks_produced,
        blocks_missed: record.blocks_missed(),
        block_production_rate: ratio(
            record.current_epoch_blocks_produced,
            record.current_epoch_leader_slots,
        ),
        future_leader_slots: record.future_leader_slots,
        next_leader_slot: record.next_leader_slot,
        slots_until_next_leader: record.slots_until_next_leader,
        seconds_until_next_leader: record.seconds_until_next_leader,
    }
}

fn merge_block_production(target: &mut BlockProduction, update: BlockProduction) {
    for (identity, (leader_slots, blocks_produced)) in update.by_identity {
        let entry = target.by_identity.entry(identity).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(leader_slots);
        entry.1 = entry.1.saturating_add(blocks_produced);
    }
}

fn reward_epoch_block_range(
    epoch_start_slot: u64,
    slots_in_epoch: u64,
    minimum_ledger_slot: Option<u64>,
) -> Option<SlotRange> {
    if epoch_start_slot == 0 || slots_in_epoch == 0 {
        return None;
    }

    let first_slot = epoch_start_slot.checked_sub(slots_in_epoch)?;
    let last_slot = epoch_start_slot.checked_sub(1)?;

    if let Some(minimum_ledger_slot) = minimum_ledger_slot {
        if first_slot < minimum_ledger_slot {
            return None;
        }
    }

    Some(SlotRange {
        first_slot,
        last_slot,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EconomicsTarget {
    identity: String,
    vote_account: String,
}

fn resolve_economics_target(
    config: &Config,
    vote_accounts: &VoteAccounts,
) -> Result<EconomicsTarget> {
    let configured = config
        .modes
        .economics
        .validator
        .as_ref()
        .context("economics mode requires a configured validator")?;
    let target = vote_accounts
        .current
        .iter()
        .chain(vote_accounts.delinquent.iter())
        .find(|info| {
            configured
                .identity
                .as_ref()
                .is_none_or(|identity| identity == &info.node_pubkey)
                && configured
                    .vote_account
                    .as_ref()
                    .is_none_or(|vote_account| vote_account == &info.vote_pubkey)
        })
        .context("configured economics validator is not present in getVoteAccounts")?;

    Ok(EconomicsTarget {
        identity: target.node_pubkey.clone(),
        vote_account: target.vote_pubkey.clone(),
    })
}

fn build_economics_metrics(
    target: &EconomicsTarget,
    balances: AccountBalances,
) -> EconomicsMetrics {
    let vat_reserve_lamports = balances
        .vote_account_rent_exempt_minimum_lamports
        .map(|minimum| balances.vote_account_lamports.saturating_sub(minimum));

    EconomicsMetrics {
        identity: target.identity.clone(),
        vote_account: target.vote_account.clone(),
        identity_balance_lamports: balances.identity_lamports,
        vote_account_balance_lamports: balances.vote_account_lamports,
        vote_account_rent_exempt_minimum_lamports: balances
            .vote_account_rent_exempt_minimum_lamports,
        vat_reserve_lamports,
        vat_epochs_available: vat_reserve_lamports.map(|reserve| reserve / VAT_LAMPORTS_PER_EPOCH),
        vat_next_epoch_funded: vat_reserve_lamports
            .map(|reserve| reserve >= VAT_LAMPORTS_PER_EPOCH),
    }
}

fn estimated_epoch_end_unixtime(
    slots_remaining: u64,
    average_slot_time_seconds: Option<f64>,
) -> Option<f64> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs_f64();
    estimated_epoch_end_unixtime_at(slots_remaining, average_slot_time_seconds, now)
}

fn estimated_epoch_end_unixtime_at(
    slots_remaining: u64,
    average_slot_time_seconds: Option<f64>,
    now_unixtime: f64,
) -> Option<f64> {
    average_slot_time_seconds.map(|slot_time| now_unixtime + slots_remaining as f64 * slot_time)
}

fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        return 0.0;
    }
    (numerator as f64 / denominator as f64) * 100.0
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GetBlockProductionConfig<'a> {
    commitment: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    range: Option<SlotRange>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GetInflationRewardConfig<'a> {
    commitment: &'a str,
    epoch: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SlotRange {
    first_slot: u64,
    last_slot: u64,
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
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcValueResponse<T> {
    value: T,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecentPerformanceSample {
    num_slots: u64,
    sample_period_secs: u16,
}

#[derive(Deserialize)]
struct AccountValue {
    lamports: u64,
    space: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EpochInfo {
    epoch: u64,
    slot_index: u64,
    slots_in_epoch: u64,
    absolute_slot: u64,
}

#[derive(Deserialize)]
pub(crate) struct VoteAccounts {
    current: Vec<VoteAccountInfo>,
    delinquent: Vec<VoteAccountInfo>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VoteAccountInfo {
    vote_pubkey: String,
    node_pubkey: String,
    activated_stake: u64,
    commission: u8,
    epoch_credits: Vec<(u64, u64, u64)>,
    last_vote: u64,
    root_slot: u64,
}

#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BlockProduction {
    by_identity: HashMap<String, (u64, u64)>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InflationRewardValue {
    amount: i64,
    epoch: u64,
}

#[cfg(test)]
mod tests {
    use super::{
        aggregate_records, build_economics_metrics, build_record, build_watched_validator_metrics,
        collect_snapshot, ensure_economics_supported, estimated_epoch_end_unixtime_at, ratio,
        resolve_economics_target, reward_epoch_block_range, AccountBalances, BlockProduction,
        EconomicsTarget, EpochCreditsSample, GetBlockProductionConfig, RecordBuildContext,
        RewardSample, RpcClient, RpcResponseSnapshot, RpcResponseStats, RpcValueResponse,
        ScopeMetrics, SlotRange, ValidatorRecord, ValidatorStatus, VoteAccountInfo, VoteAccounts,
        WatchedValidatorMetrics, MAX_BLOCK_PRODUCTION_RANGE_SLOTS,
    };
    use crate::{
        config::{
            Config, ConnectorConfig, ConsensusCommands, ConsensusModeConfig, EconomicsCommands,
            EconomicsModeConfig, HealthCommands, HealthModeConfig, LeadershipCommands,
            LeadershipModeConfig, ModesConfig, WatchedValidatorConfig,
        },
        labels::LabelMatcher,
        mode::{CommandId, ModeId, ProviderFeature, SolanaRpcCompatibility},
        source::SourceChain,
    };
    use anyhow::{anyhow, Context, Result};
    use axum::{
        extract::{Json, State},
        http::StatusCode,
        response::IntoResponse,
        routing::post,
        Router,
    };
    use serde_json::{json, Value};
    use std::{collections::HashMap, sync::Arc, time::Duration};
    use tokio::{net::TcpListener, sync::Mutex, task::JoinHandle};

    #[derive(Clone, Copy)]
    enum MockRpcProfile {
        Default,
        StrictCapabilityParams,
        NoAccountSpace,
        NoCurrentBlockProduction,
        Unhealthy,
        NoEpochInfo,
        NoInflationRewards,
        NoLeaderSchedule,
        NoMinimumLedgerSlot,
        NoPerformanceSamples,
        NoSlotLeader,
        NullPerformanceSamples,
        MissingPerformanceSamplesResult,
    }

    #[derive(Clone)]
    struct MockRpcState {
        profile: MockRpcProfile,
        methods: Arc<Mutex<Vec<String>>>,
    }

    #[test]
    fn rpc_response_stats_classify_bounded_statuses() {
        let responses = RpcResponseStats::default();
        for status in [
            StatusCode::OK,
            StatusCode::NO_CONTENT,
            StatusCode::TEMPORARY_REDIRECT,
            StatusCode::FORBIDDEN,
            StatusCode::TOO_MANY_REQUESTS,
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::BAD_GATEWAY,
        ] {
            responses.record(status);
        }
        responses.record_failure();

        assert_eq!(
            responses.snapshot(),
            RpcResponseSnapshot {
                total: 7,
                successful_2xx: 2,
                redirection_3xx: 1,
                client_error_4xx: 2,
                server_error_5xx: 2,
                status_200: 1,
                status_403: 1,
                status_429: 1,
                status_500: 1,
                failed_attempts: 1,
                ..RpcResponseSnapshot::default()
            }
        );
    }

    #[test]
    fn aggregates_by_scope() -> Result<()> {
        let records = vec![
            ValidatorRecord {
                identity: "id-1".to_string(),
                vote_account: "vote-1".to_string(),
                status: ValidatorStatus::Current,
                activated_stake_lamports: 10,
                commission: 5,
                last_vote: 95,
                root_slot: 94,
                latest_epoch_credits: Some(EpochCreditsSample {
                    epoch: 9,
                    earned: 10,
                }),
                latest_epoch_reward: Some(RewardSample {
                    epoch: 9,
                    amount_lamports: 9_000_000_000,
                }),
                latest_epoch_rewarded_blocks_produced: 3,
                current_epoch_leader_slots: 4,
                current_epoch_blocks_produced: 3,
                future_leader_slots: 2,
                next_leader_slot: Some(105),
                slots_until_next_leader: Some(5),
                seconds_until_next_leader: Some(2.0),
                scopes: vec!["scope:watched".to_string(), "scope:all-network".to_string()],
            },
            ValidatorRecord {
                identity: "id-2".to_string(),
                vote_account: "vote-2".to_string(),
                status: ValidatorStatus::Delinquent,
                activated_stake_lamports: 5,
                commission: 10,
                last_vote: 90,
                root_slot: 88,
                latest_epoch_credits: Some(EpochCreditsSample {
                    epoch: 9,
                    earned: 5,
                }),
                latest_epoch_reward: Some(RewardSample {
                    epoch: 9,
                    amount_lamports: 2_000_000_000,
                }),
                latest_epoch_rewarded_blocks_produced: 1,
                current_epoch_leader_slots: 2,
                current_epoch_blocks_produced: 1,
                future_leader_slots: 1,
                next_leader_slot: None,
                slots_until_next_leader: None,
                seconds_until_next_leader: None,
                scopes: vec!["scope:network".to_string(), "scope:all-network".to_string()],
            },
        ];

        let snapshot = aggregate_records("mainnet-beta".to_string(), 10, 100, &records);
        let watched = snapshot
            .scopes
            .get("scope:watched")
            .ok_or_else(|| anyhow!("missing watched scope"))?;
        let all_network = snapshot
            .scopes
            .get("scope:all-network")
            .ok_or_else(|| anyhow!("missing all-network scope"))?;

        assert_eq!(snapshot.vote_credits_sample_epoch, Some(9));
        assert_eq!(snapshot.reward_sample_epoch, Some(9));
        assert_eq!(
            watched,
            &ScopeMetrics {
                current_count: 1,
                delinquent_count: 0,
                current_stake_lamports: 10,
                delinquent_stake_lamports: 0,
                latest_epoch_vote_credits: Some(10),
                latest_epoch_reward_lamports: 9_000_000_000,
                latest_epoch_rewarded_blocks_produced: 3,
                vote_efficiency_pct: Some(100.0),
                last_vote_slot_lag: 5.0,
                root_slot_lag: 6.0,
                average_commission_pct: 5.0,
                blocks_produced: 3,
                blocks_missed: 1,
                block_production_rate: 75.0,
                future_leader_slots: 2,
            }
        );

        assert_eq!(all_network.current_count, 1);
        assert_eq!(all_network.delinquent_count, 1);
        assert_eq!(all_network.latest_epoch_vote_credits, Some(15));
        assert!(all_network
            .vote_efficiency_pct
            .is_some_and(|efficiency| efficiency > 80.0));
        assert_eq!(
            snapshot.watched_validators,
            vec![WatchedValidatorMetrics {
                identity: "id-1".to_string(),
                vote_account: "vote-1".to_string(),
                status: ValidatorStatus::Current,
                scopes: vec!["scope:watched".to_string(), "scope:all-network".to_string()],
                activated_stake_lamports: 10,
                latest_epoch_vote_credits: Some(10),
                latest_epoch_reward_lamports: 9_000_000_000,
                latest_epoch_rewarded_blocks_produced: 3,
                vote_efficiency_pct: Some(100.0),
                last_vote_slot_lag: 5,
                root_slot_lag: 6,
                commission_pct: 5,
                blocks_produced: 3,
                blocks_missed: 1,
                block_production_rate: 75.0,
                future_leader_slots: 2,
                next_leader_slot: Some(105),
                slots_until_next_leader: Some(5),
                seconds_until_next_leader: Some(2.0),
            }]
        );

        Ok(())
    }

    #[test]
    fn watched_validator_metrics_omit_efficiency_without_matching_epoch() {
        let record = ValidatorRecord {
            identity: "id-1".to_string(),
            vote_account: "vote-1".to_string(),
            status: ValidatorStatus::Current,
            activated_stake_lamports: 10,
            commission: 7,
            last_vote: 98,
            root_slot: 97,
            latest_epoch_credits: Some(EpochCreditsSample {
                epoch: 4,
                earned: 12,
            }),
            latest_epoch_reward: None,
            latest_epoch_rewarded_blocks_produced: 0,
            current_epoch_leader_slots: 0,
            current_epoch_blocks_produced: 0,
            future_leader_slots: 3,
            next_leader_slot: Some(103),
            slots_until_next_leader: Some(3),
            seconds_until_next_leader: Some(1.2),
            scopes: vec!["scope:watched".to_string()],
        };

        let watched = build_watched_validator_metrics(&record, 100, Some(20), Some(5), None);
        assert_eq!(
            watched,
            WatchedValidatorMetrics {
                identity: "id-1".to_string(),
                vote_account: "vote-1".to_string(),
                status: ValidatorStatus::Current,
                scopes: vec!["scope:watched".to_string()],
                activated_stake_lamports: 10,
                latest_epoch_vote_credits: None,
                latest_epoch_reward_lamports: 0,
                latest_epoch_rewarded_blocks_produced: 0,
                vote_efficiency_pct: None,
                last_vote_slot_lag: 2,
                root_slot_lag: 3,
                commission_pct: 7,
                blocks_produced: 0,
                blocks_missed: 0,
                block_production_rate: 0.0,
                future_leader_slots: 3,
                next_leader_slot: Some(103),
                slots_until_next_leader: Some(3),
                seconds_until_next_leader: Some(1.2),
            }
        );
    }

    #[test]
    fn ratio_returns_zero_for_empty_denominator() {
        assert_eq!(ratio(0, 0), 0.0);
        assert_eq!(ratio(5, 0), 0.0);
        assert_eq!(ratio(3, 4), 75.0);
    }

    #[tokio::test]
    async fn get_block_production_for_range_batches_large_requests() -> Result<()> {
        let (rpc_url, server_task) = start_mock_rpc_server().await?;
        let client = RpcClient::new(
            rpc_url,
            Duration::from_secs(5),
            "confirmed".to_string(),
            crate::collector::RpcResponseStats::default(),
        )?;

        let block_production = client.get_block_production_for_range(700, 10_799).await?;

        server_task.abort();

        assert_eq!(block_production.by_identity.get("id-1"), Some(&(7, 6)));

        Ok(())
    }

    #[tokio::test]
    async fn call_optional_surfaces_json_rpc_errors_from_http_400() -> Result<()> {
        let (rpc_url, server_task) = start_mock_rpc_server().await?;
        let responses = RpcResponseStats::default();
        let client = RpcClient::new(
            rpc_url,
            Duration::from_secs(5),
            "confirmed".to_string(),
            responses.clone(),
        )?;

        let err = match client
            .call_optional::<RpcValueResponse<BlockProduction>, _>(
                "getBlockProduction",
                [GetBlockProductionConfig {
                    commitment: "confirmed",
                    range: Some(SlotRange {
                        first_slot: 1,
                        last_slot: 5_001,
                    }),
                }],
            )
            .await
        {
            Ok(_) => panic!("expected oversized block production request to fail"),
            Err(err) => err,
        };

        server_task.abort();

        assert!(err
            .to_string()
            .contains("getBlockProduction failed with JSON-RPC code -32600"));
        assert_eq!(responses.snapshot().client_error_4xx, 1);
        assert_eq!(responses.snapshot().failed_attempts, 1);

        Ok(())
    }

    #[tokio::test]
    async fn rpc_client_collect_snapshot_and_validate_economics_against_mock_rpc() -> Result<()> {
        let (rpc_url, server_task) = start_mock_rpc_server().await?;
        let mut config = Config {
            network: "devnet".to_string(),
            connectors: vec![ConnectorConfig {
                name: "primary".to_string(),
                provider: "mock-solana".to_string(),
                features: vec![ProviderFeature::HistoricalData],
                rpc_url: Some(rpc_url.clone()),
                rpc_timeout_sec: 5,
                ..ConnectorConfig::default()
            }],
            modes: modes_for(&["primary"], true),
            watched_validators: vec![WatchedValidatorConfig {
                identity: Some("id-1".to_string()),
                vote_account: Some("vote-1".to_string()),
                labels: vec!["operator:test".to_string()],
            }],
            ..Config::default()
        };
        config.modes.economics.validator = Some(WatchedValidatorConfig {
            identity: Some("id-1".to_string()),
            vote_account: Some("vote-1".to_string()),
            labels: vec!["focus:economics".to_string()],
        });
        let client = RpcClient::new(
            rpc_url,
            Duration::from_secs(5),
            config.connectors[0].commitment_config()?,
            crate::collector::RpcResponseStats::default(),
        )?;

        client.ensure_healthy().await?;
        let sources =
            SourceChain::from_config(&config, crate::collector::RpcResponseStats::default())?;
        ensure_economics_supported(&sources, &config).await?;
        let snapshot = collect_snapshot(&sources, &config).await?;

        server_task.abort();

        assert_eq!(snapshot.network, "devnet");
        assert_eq!(snapshot.epoch, 9);
        assert_eq!(snapshot.slot, 810);
        assert_eq!(snapshot.vote_credits_sample_epoch, Some(8));
        assert_eq!(snapshot.reward_sample_epoch, Some(8));
        assert_eq!(snapshot.watched_validators.len(), 1);
        assert_eq!(
            snapshot.watched_validators[0].latest_epoch_reward_lamports,
            12_000_000_000
        );
        assert_eq!(
            snapshot.watched_validators[0].latest_epoch_rewarded_blocks_produced,
            3
        );
        assert_eq!(snapshot.watched_validators[0].blocks_produced, 4);
        assert_eq!(snapshot.watched_validators[0].future_leader_slots, 3);

        assert_eq!(snapshot.epoch_progress_pct, 10.0);
        assert_eq!(snapshot.epoch_slots_remaining, 90);
        assert_eq!(snapshot.average_slot_time_seconds, Some(0.4));
        assert!(snapshot.estimated_epoch_end_unixtime.is_some());
        assert_eq!(snapshot.watched_validators[0].next_leader_slot, Some(810));
        assert_eq!(
            snapshot.watched_validators[0].slots_until_next_leader,
            Some(0)
        );
        assert_eq!(
            snapshot.watched_validators[0].seconds_until_next_leader,
            Some(0.0)
        );
        let economics = snapshot
            .economics
            .as_ref()
            .ok_or_else(|| anyhow!("missing economics metrics"))?;
        assert_eq!(economics.identity_balance_lamports, 3_000_000_000);
        assert_eq!(economics.vote_account_balance_lamports, 2_500_000_000);
        assert_eq!(
            economics.vote_account_rent_exempt_minimum_lamports,
            Some(900_000_000)
        );
        assert_eq!(economics.vat_reserve_lamports, Some(1_600_000_000));
        assert_eq!(economics.vat_epochs_available, Some(2));
        assert_eq!(economics.vat_next_epoch_funded, Some(true));
        Ok(())
    }

    #[tokio::test]
    async fn missing_performance_samples_omit_time_estimates_without_failing_collection(
    ) -> Result<()> {
        let (rpc_url, server_task) =
            start_mock_rpc_server_with_profile(MockRpcProfile::NoPerformanceSamples).await?;
        let config = Config {
            network: "devnet".to_string(),
            connectors: vec![ConnectorConfig {
                name: "primary".to_string(),
                provider: "mock-solana".to_string(),
                features: Vec::new(),
                rpc_url: Some(rpc_url),
                rpc_timeout_sec: 5,
                ..ConnectorConfig::default()
            }],
            modes: modes_for(&["primary"], false),
            watched_validators: vec![WatchedValidatorConfig {
                identity: Some("id-1".to_string()),
                vote_account: Some("vote-1".to_string()),
                labels: vec!["operator:test".to_string()],
            }],
            ..Config::default()
        };
        let sources =
            SourceChain::from_config(&config, crate::collector::RpcResponseStats::default())?;
        let snapshot = collect_snapshot(&sources, &config).await?;
        server_task.abort();

        assert_eq!(snapshot.average_slot_time_seconds, None);
        assert_eq!(snapshot.estimated_epoch_end_unixtime, None);
        assert_eq!(snapshot.watched_validators[0].next_leader_slot, Some(810));
        assert_eq!(
            snapshot.watched_validators[0].seconds_until_next_leader,
            None
        );
        assert!(!snapshot
            .data_sources
            .iter()
            .any(|source| source.command == CommandId::RecentPerformanceSamples));
        Ok(())
    }

    #[tokio::test]
    async fn collect_snapshot_with_connectors_uses_fallback_connector_for_missing_optional_metric(
    ) -> Result<()> {
        let (primary_rpc_url, primary_server_task) =
            start_mock_rpc_server_with_profile(MockRpcProfile::NoLeaderSchedule).await?;
        let (fallback_rpc_url, fallback_server_task) = start_mock_rpc_server().await?;
        let config = Config {
            network: "devnet".to_string(),
            connectors: vec![
                ConnectorConfig {
                    name: "local-validator".to_string(),
                    provider: "solana-validator".to_string(),
                    features: Vec::new(),
                    rpc_url: Some(primary_rpc_url),
                    rpc_timeout_sec: 5,
                    ..ConnectorConfig::default()
                },
                ConnectorConfig {
                    name: "helius-fallback".to_string(),
                    provider: "helius".to_string(),
                    features: Vec::new(),
                    rpc_url: Some(fallback_rpc_url),
                    rpc_timeout_sec: 5,
                    ..ConnectorConfig::default()
                },
            ],
            modes: modes_for(&["local-validator", "helius-fallback"], false),
            watched_validators: vec![WatchedValidatorConfig {
                identity: Some("id-1".to_string()),
                vote_account: Some("vote-1".to_string()),
                labels: vec!["operator:test".to_string()],
            }],
            ..Config::default()
        };
        let sources =
            SourceChain::from_config(&config, crate::collector::RpcResponseStats::default())?;
        let snapshot = collect_snapshot(&sources, &config).await?;

        primary_server_task.abort();
        fallback_server_task.abort();

        let epoch_info_source = snapshot
            .data_sources
            .iter()
            .find(|source| source.command == CommandId::EpochInfo)
            .ok_or_else(|| anyhow!("missing epoch-info source"))?;
        let current_schedule_source = snapshot
            .data_sources
            .iter()
            .find(|source| source.command == CommandId::CurrentLeaderSchedule)
            .ok_or_else(|| anyhow!("missing current-leader-schedule source"))?;
        let next_schedule_source = snapshot
            .data_sources
            .iter()
            .find(|source| source.command == CommandId::NextLeaderSchedule)
            .ok_or_else(|| anyhow!("missing next-leader-schedule source"))?;

        assert_eq!(epoch_info_source.connector.as_ref(), "local-validator");
        assert_eq!(epoch_info_source.provider.as_ref(), "solana-validator");
        assert_eq!(
            current_schedule_source.connector.as_ref(),
            "helius-fallback"
        );
        assert_eq!(current_schedule_source.provider.as_ref(), "helius");
        assert_eq!(current_schedule_source.precedence, 2);
        assert_eq!(next_schedule_source.connector.as_ref(), "helius-fallback");
        assert_eq!(snapshot.watched_validators[0].future_leader_slots, 3);

        Ok(())
    }

    #[tokio::test]
    async fn collect_snapshot_with_connectors_uses_fallback_connector_for_missing_required_metric(
    ) -> Result<()> {
        let (primary_rpc_url, primary_server_task) =
            start_mock_rpc_server_with_profile(MockRpcProfile::NoEpochInfo).await?;
        let (fallback_rpc_url, fallback_server_task) = start_mock_rpc_server().await?;
        let config = Config {
            network: "devnet".to_string(),
            connectors: vec![
                ConnectorConfig {
                    name: "local-validator".to_string(),
                    provider: "solana-validator".to_string(),
                    features: Vec::new(),
                    rpc_url: Some(primary_rpc_url),
                    rpc_timeout_sec: 5,
                    ..ConnectorConfig::default()
                },
                ConnectorConfig {
                    name: "helius-fallback".to_string(),
                    provider: "helius".to_string(),
                    features: Vec::new(),
                    rpc_url: Some(fallback_rpc_url),
                    rpc_timeout_sec: 5,
                    ..ConnectorConfig::default()
                },
            ],
            modes: modes_for(&["local-validator", "helius-fallback"], false),
            ..Config::default()
        };
        let sources =
            SourceChain::from_config(&config, crate::collector::RpcResponseStats::default())?;
        let snapshot = collect_snapshot(&sources, &config).await?;

        primary_server_task.abort();
        fallback_server_task.abort();

        let epoch_info_source = snapshot
            .data_sources
            .iter()
            .find(|source| source.command == CommandId::EpochInfo)
            .ok_or_else(|| anyhow!("missing epoch-info source"))?;
        let vote_accounts_source = snapshot
            .data_sources
            .iter()
            .find(|source| source.command == CommandId::VoteAccounts)
            .ok_or_else(|| anyhow!("missing vote-accounts source"))?;

        assert_eq!(epoch_info_source.connector.as_ref(), "helius-fallback");
        assert_eq!(epoch_info_source.provider.as_ref(), "helius");
        assert_eq!(epoch_info_source.precedence, 2);
        assert_eq!(vote_accounts_source.connector.as_ref(), "local-validator");
        assert_eq!(vote_accounts_source.provider.as_ref(), "solana-validator");

        Ok(())
    }

    #[tokio::test]
    async fn collect_snapshot_resolves_each_command_from_its_own_route() -> Result<()> {
        let (first_rpc_url, first_server_task) = start_mock_rpc_server().await?;
        let (second_rpc_url, second_server_task) = start_mock_rpc_server().await?;
        let mut modes = modes_for(&["first", "second"], false);
        modes.consensus.commands.vote_accounts = vec!["second".to_string(), "first".to_string()];
        let config = Config {
            network: "devnet".to_string(),
            connectors: vec![
                ConnectorConfig {
                    name: "first".to_string(),
                    provider: "solana-validator".to_string(),
                    features: Vec::new(),
                    rpc_url: Some(first_rpc_url),
                    rpc_timeout_sec: 5,
                    ..ConnectorConfig::default()
                },
                ConnectorConfig {
                    name: "second".to_string(),
                    provider: "helius".to_string(),
                    features: Vec::new(),
                    rpc_url: Some(second_rpc_url),
                    rpc_timeout_sec: 5,
                    ..ConnectorConfig::default()
                },
            ],
            modes,
            ..Config::default()
        };
        let sources =
            SourceChain::from_config(&config, crate::collector::RpcResponseStats::default())?;
        let snapshot = collect_snapshot(&sources, &config).await?;

        first_server_task.abort();
        second_server_task.abort();

        let epoch_info_source = snapshot
            .data_sources
            .iter()
            .find(|source| source.command == CommandId::EpochInfo)
            .ok_or_else(|| anyhow!("missing epoch-info source"))?;
        let vote_accounts_source = snapshot
            .data_sources
            .iter()
            .find(|source| source.command == CommandId::VoteAccounts)
            .ok_or_else(|| anyhow!("missing vote-accounts source"))?;

        assert_eq!(epoch_info_source.connector.as_ref(), "first");
        assert_eq!(epoch_info_source.precedence, 1);
        assert_eq!(vote_accounts_source.connector.as_ref(), "second");
        assert_eq!(vote_accounts_source.precedence, 1);
        Ok(())
    }

    #[tokio::test]
    async fn required_command_reports_the_last_failed_route_connector() -> Result<()> {
        let (first_rpc_url, first_server_task) =
            start_mock_rpc_server_with_profile(MockRpcProfile::NoEpochInfo).await?;
        let (second_rpc_url, second_server_task) =
            start_mock_rpc_server_with_profile(MockRpcProfile::NoEpochInfo).await?;
        let config = Config {
            network: "devnet".to_string(),
            connectors: vec![
                ConnectorConfig {
                    name: "first".to_string(),
                    provider: "solana-validator".to_string(),
                    features: Vec::new(),
                    rpc_url: Some(first_rpc_url),
                    rpc_timeout_sec: 5,
                    ..ConnectorConfig::default()
                },
                ConnectorConfig {
                    name: "second".to_string(),
                    provider: "helius".to_string(),
                    features: Vec::new(),
                    rpc_url: Some(second_rpc_url),
                    rpc_timeout_sec: 5,
                    ..ConnectorConfig::default()
                },
            ],
            modes: modes_for(&["first", "second"], false),
            ..Config::default()
        };
        let sources =
            SourceChain::from_config(&config, crate::collector::RpcResponseStats::default())?;

        let Err(error) = collect_snapshot(&sources, &config).await else {
            anyhow::bail!("all failed required command routes must fail collection");
        };
        first_server_task.abort();
        second_server_task.abort();

        assert!(format!("{error:#}").contains(
            "all connectors failed for required command epoch-info; last connector was second"
        ));
        Ok(())
    }

    #[tokio::test]
    async fn economics_succeeds_when_optional_leadership_command_is_unavailable() -> Result<()> {
        let (rpc_url, server_task) =
            start_mock_rpc_server_with_profile(MockRpcProfile::NoCurrentBlockProduction).await?;
        let config = Config {
            network: "devnet".to_string(),
            connectors: vec![ConnectorConfig {
                name: "primary".to_string(),
                provider: "mock-solana".to_string(),
                features: vec![ProviderFeature::HistoricalData],
                rpc_url: Some(rpc_url),
                rpc_timeout_sec: 5,
                ..ConnectorConfig::default()
            }],
            modes: modes_for(&["primary"], true),
            watched_validators: vec![WatchedValidatorConfig {
                identity: Some("id-1".to_string()),
                vote_account: Some("vote-1".to_string()),
                labels: vec!["operator:test".to_string()],
            }],
            ..Config::default()
        };
        let sources =
            SourceChain::from_config(&config, crate::collector::RpcResponseStats::default())?;

        ensure_economics_supported(&sources, &config).await?;
        let snapshot = collect_snapshot(&sources, &config).await?;
        server_task.abort();

        assert!(snapshot
            .mode_statuses
            .iter()
            .any(|status| status.mode == ModeId::Leadership && !status.up));
        assert!(snapshot
            .mode_statuses
            .iter()
            .any(|status| status.mode == ModeId::Economics && status.up));
        assert!(!snapshot
            .data_sources
            .iter()
            .any(|source| source.command == CommandId::CurrentBlockProduction));
        assert!(snapshot
            .data_sources
            .iter()
            .any(|source| source.command == CommandId::RewardEpochBlockProduction));
        Ok(())
    }

    #[tokio::test]
    async fn get_inflation_rewards_batches_requests_at_rpc_limit() -> Result<()> {
        let (rpc_url, server_task) = start_mock_rpc_server().await?;
        let client = RpcClient::new(
            rpc_url,
            Duration::from_secs(5),
            "confirmed".to_string(),
            crate::collector::RpcResponseStats::default(),
        )?;
        let addresses = (0..205)
            .map(|index| format!("vote-{index}"))
            .collect::<Vec<_>>();

        let rewards = client.get_inflation_rewards(&addresses, 8).await?;

        server_task.abort();

        assert_eq!(rewards.len(), 205);
        assert_eq!(
            rewards.get("vote-0"),
            Some(&RewardSample {
                epoch: 8,
                amount_lamports: 12_000_000_000,
            })
        );
        assert_eq!(
            rewards.get("vote-204"),
            Some(&RewardSample {
                epoch: 8,
                amount_lamports: 12_000_000_000,
            })
        );

        Ok(())
    }

    #[test]
    fn build_record_uses_block_production_schedules_and_labels() {
        let config = Config {
            network: "devnet".to_string(),
            connectors: vec![ConnectorConfig {
                name: "primary".to_string(),
                provider: "solana-validator".to_string(),
                features: Vec::new(),
                rpc_url: Some("https://rpc.example".to_string()),
                ws_url: Some("wss://rpc.example".to_string()),
                ..ConnectorConfig::default()
            }],
            delinquent_slot_distance: Some(128),
            watched_validators: vec![WatchedValidatorConfig {
                identity: None,
                vote_account: Some("vote-1".to_string()),
                labels: vec!["operator:test".to_string()],
            }],
            ..Config::default()
        };
        let matcher = LabelMatcher::from_config(&config);
        let mut by_identity = HashMap::new();
        let mut reward_epoch_by_identity = HashMap::new();
        let mut current_schedule = HashMap::new();
        let mut next_schedule = HashMap::new();
        let mut latest_epoch_rewards = HashMap::new();

        by_identity.insert("id-1".to_string(), (5, 4));
        reward_epoch_by_identity.insert("id-1".to_string(), (4, 3));
        current_schedule.insert("id-1".to_string(), vec![5, 10, 15]);
        next_schedule.insert("id-1".to_string(), vec![1, 2]);
        latest_epoch_rewards.insert(
            "vote-1".to_string(),
            RewardSample {
                epoch: 9,
                amount_lamports: 12_000_000_000,
            },
        );

        let context = RecordBuildContext {
            matcher: &matcher,
            current_epoch: 9,
            current_slot: 110,
            epoch_start_slot: 100,
            next_epoch_start_slot: 200,
            average_slot_time_seconds: Some(0.5),
            by_identity: &by_identity,
            reward_epoch_by_identity: &reward_epoch_by_identity,
            current_schedule: Some(&current_schedule),
            next_schedule: Some(&next_schedule),
            latest_epoch_rewards: &latest_epoch_rewards,
        };

        let record = build_record(
            VoteAccountInfo {
                vote_pubkey: "vote-1".to_string(),
                node_pubkey: "id-1".to_string(),
                activated_stake: 42,
                commission: 7,
                epoch_credits: vec![(8, 20, 12), (9, 35, 20), (u64::MAX, u64::MAX, u64::MAX)],
                last_vote: 109,
                root_slot: 108,
            },
            ValidatorStatus::Current,
            &context,
        );

        assert_eq!(record.current_epoch_leader_slots, 5);
        assert_eq!(record.current_epoch_blocks_produced, 4);
        assert_eq!(record.latest_epoch_rewarded_blocks_produced, 3);
        assert_eq!(record.future_leader_slots, 4);
        assert_eq!(record.next_leader_slot, Some(110));
        assert_eq!(record.slots_until_next_leader, Some(0));
        assert_eq!(record.seconds_until_next_leader, Some(0.0));
        assert_eq!(
            record.latest_epoch_credits,
            Some(EpochCreditsSample {
                epoch: 9,
                earned: 15,
            })
        );
        assert_eq!(
            record.latest_epoch_reward,
            Some(RewardSample {
                epoch: 9,
                amount_lamports: 12_000_000_000,
            })
        );
        assert!(record.scopes.contains(&"scope:watched".to_string()));
        assert!(record.scopes.contains(&"operator:test".to_string()));
    }

    #[test]
    fn build_record_defaults_missing_block_data_to_zero() {
        let config = Config {
            network: "devnet".to_string(),
            connectors: vec![ConnectorConfig {
                name: "primary".to_string(),
                provider: "solana-validator".to_string(),
                features: Vec::new(),
                rpc_url: Some("https://rpc.example".to_string()),
                ..ConnectorConfig::default()
            }],
            ..Config::default()
        };
        let matcher = LabelMatcher::from_config(&config);
        let by_identity = HashMap::new();
        let reward_epoch_by_identity = HashMap::new();
        let latest_epoch_rewards = HashMap::new();

        let context = RecordBuildContext {
            matcher: &matcher,
            current_epoch: 1,
            current_slot: 50,
            epoch_start_slot: 40,
            next_epoch_start_slot: 140,
            average_slot_time_seconds: None,
            by_identity: &by_identity,
            reward_epoch_by_identity: &reward_epoch_by_identity,
            current_schedule: None,
            next_schedule: None,
            latest_epoch_rewards: &latest_epoch_rewards,
        };

        let record = build_record(
            VoteAccountInfo {
                vote_pubkey: "vote-2".to_string(),
                node_pubkey: "id-2".to_string(),
                activated_stake: 0,
                commission: 0,
                epoch_credits: vec![],
                last_vote: 49,
                root_slot: 48,
            },
            ValidatorStatus::Delinquent,
            &context,
        );

        assert_eq!(record.current_epoch_leader_slots, 0);
        assert_eq!(record.current_epoch_blocks_produced, 0);
        assert_eq!(record.future_leader_slots, 0);
        assert_eq!(record.next_leader_slot, None);
        assert_eq!(record.slots_until_next_leader, None);
        assert_eq!(record.seconds_until_next_leader, None);
        assert_eq!(record.latest_epoch_credits, None);
        assert_eq!(record.latest_epoch_reward, None);
        assert!(record.scopes.contains(&"scope:network".to_string()));
    }

    #[test]
    fn reward_epoch_block_range_skips_pruned_history() {
        assert!(reward_epoch_block_range(100, 20, Some(81)).is_none());
        assert!(matches!(
            reward_epoch_block_range(100, 20, Some(80)),
            Some(SlotRange {
                first_slot: 80,
                last_slot: 99,
            })
        ));
        assert!(reward_epoch_block_range(0, 20, Some(0)).is_none());
    }

    #[test]
    fn economics_target_resolves_vote_account_from_configured_identity() -> Result<()> {
        let config = Config {
            modes: ModesConfig {
                economics: EconomicsModeConfig {
                    enabled: true,
                    validator: Some(crate::config::WatchedValidatorConfig {
                        identity: Some("id-1".to_string()),
                        vote_account: None,
                        labels: vec![],
                    }),
                    ..EconomicsModeConfig::default()
                },
                ..ModesConfig::default()
            },
            ..Config::default()
        };
        let vote_accounts = VoteAccounts {
            current: vec![],
            delinquent: vec![VoteAccountInfo {
                vote_pubkey: "vote-1".to_string(),
                node_pubkey: "id-1".to_string(),
                activated_stake: 0,
                commission: 0,
                epoch_credits: vec![],
                last_vote: 0,
                root_slot: 0,
            }],
        };

        assert_eq!(
            resolve_economics_target(&config, &vote_accounts)?,
            EconomicsTarget {
                identity: "id-1".to_string(),
                vote_account: "vote-1".to_string(),
            }
        );
        Ok(())
    }

    #[test]
    fn economics_target_rejects_mismatched_identity_and_vote_account() {
        let config = Config {
            modes: ModesConfig {
                economics: EconomicsModeConfig {
                    enabled: true,
                    validator: Some(crate::config::WatchedValidatorConfig {
                        identity: Some("id-1".to_string()),
                        vote_account: Some("wrong-vote".to_string()),
                        labels: vec![],
                    }),
                    ..EconomicsModeConfig::default()
                },
                ..ModesConfig::default()
            },
            ..Config::default()
        };
        let vote_accounts = VoteAccounts {
            current: vec![VoteAccountInfo {
                vote_pubkey: "vote-1".to_string(),
                node_pubkey: "id-1".to_string(),
                activated_stake: 0,
                commission: 0,
                epoch_credits: vec![],
                last_vote: 0,
                root_slot: 0,
            }],
            delinquent: vec![],
        };

        let error = resolve_economics_target(&config, &vote_accounts)
            .err()
            .map(|error| error.to_string());
        assert_eq!(
            error.as_deref(),
            Some("configured economics validator is not present in getVoteAccounts")
        );
    }

    #[test]
    fn economics_metrics_preserve_rent_and_vat_boundaries() {
        let metrics = build_economics_metrics(
            &EconomicsTarget {
                identity: "id-1".to_string(),
                vote_account: "vote-1".to_string(),
            },
            AccountBalances {
                identity_lamports: 3_000_000_000,
                vote_account_lamports: 2_500_000_000,
                vote_account_rent_exempt_minimum_lamports: Some(900_000_000),
            },
        );

        assert_eq!(metrics.vat_reserve_lamports, Some(1_600_000_000));
        assert_eq!(metrics.vat_epochs_available, Some(2));
        assert_eq!(metrics.vat_next_epoch_funded, Some(true));
    }

    #[tokio::test]
    async fn missing_vote_account_space_preserves_balances_and_omits_vat() -> Result<()> {
        let (rpc_url, server_task) =
            start_mock_rpc_server_with_profile(MockRpcProfile::NoAccountSpace).await?;
        let client = RpcClient::new(
            rpc_url,
            Duration::from_secs(5),
            "confirmed".to_string(),
            crate::collector::RpcResponseStats::default(),
        )?;

        let balances = client.get_account_balances("id-1", "vote-1").await?;
        server_task.abort();
        assert_eq!(balances.identity_lamports, 3_000_000_000);
        assert_eq!(balances.vote_account_lamports, 2_500_000_000);
        assert_eq!(balances.vote_account_rent_exempt_minimum_lamports, None);

        let metrics = build_economics_metrics(
            &EconomicsTarget {
                identity: "id-1".to_string(),
                vote_account: "vote-1".to_string(),
            },
            balances,
        );
        assert_eq!(metrics.vat_reserve_lamports, None);
        assert_eq!(metrics.vat_epochs_available, None);
        assert_eq!(metrics.vat_next_epoch_funded, None);
        Ok(())
    }

    #[test]
    fn epoch_end_estimate_requires_average_slot_time() {
        assert_eq!(
            estimated_epoch_end_unixtime_at(100, Some(0.4), 1_700_000_000.0),
            Some(1_700_000_040.0)
        );
        assert_eq!(
            estimated_epoch_end_unixtime_at(100, None, 1_700_000_000.0),
            None
        );
    }

    #[tokio::test]
    async fn rpc_health_errors_do_not_expose_secret_query_parameters() -> Result<()> {
        let (rpc_url, server_task) =
            start_mock_rpc_server_with_profile(MockRpcProfile::Unhealthy).await?;
        let secret = "do-not-expose-this-api-key";
        let client = RpcClient::new(
            format!("{rpc_url}/?api-key={secret}"),
            Duration::from_secs(5),
            "confirmed".to_string(),
            crate::collector::RpcResponseStats::default(),
        )?;

        let Err(error) = client.ensure_healthy().await else {
            anyhow::bail!("unhealthy RPC should fail");
        };
        server_task.abort();
        let message = format!("{error:#}");

        assert!(message.contains("unexpected getHealth result"));
        assert!(
            !message.contains(secret),
            "RPC health errors must redact secret URL query parameters"
        );
        Ok(())
    }

    #[tokio::test]
    async fn connector_capability_probe_rejects_extended_profile_without_exposing_endpoint_secrets(
    ) -> Result<()> {
        let (rpc_url, server_task) =
            start_mock_rpc_server_with_profile(MockRpcProfile::NoPerformanceSamples).await?;
        let secret = "do-not-expose-capability-probe-key";
        let mut config = Config {
            connectors: vec![ConnectorConfig {
                name: "limited-rpc".to_string(),
                provider: "mock-provider".to_string(),
                features: Vec::new(),
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
                rpc_url: Some(format!("{rpc_url}/?api-key={secret}")),
                ..ConnectorConfig::default()
            }],
            ..Config::default()
        };
        let sources =
            SourceChain::from_config(&config, crate::collector::RpcResponseStats::default())?;
        let Err(error) = sources.verify_capabilities().await else {
            anyhow::bail!("extended compatibility without extended methods should fail");
        };
        let message = format!("{error:#}");
        assert!(message.contains("connector \"limited-rpc\""), "{message}");
        assert!(message.contains("provider \"mock-provider\""), "{message}");
        assert!(message.contains("RPC endpoint"), "{message}");
        assert!(
            message.contains("solana_rpc_compatibility = \"extended\""),
            "{message}"
        );
        assert!(message.contains("getRecentPerformanceSamples"), "{message}");
        assert!(!message.contains(secret), "{message}");

        config.connectors[0].solana_rpc_compatibility = Some(SolanaRpcCompatibility::Core);
        SourceChain::from_config(&config, crate::collector::RpcResponseStats::default())?
            .verify_capabilities()
            .await?;
        server_task.abort();
        Ok(())
    }

    #[tokio::test]
    async fn connector_capability_probe_uses_only_declared_core_methods() -> Result<()> {
        let (rpc_url, server_task) =
            start_mock_rpc_server_with_profile(MockRpcProfile::NoSlotLeader).await?;
        let config = Config {
            connectors: vec![ConnectorConfig {
                name: "core-rpc".to_string(),
                provider: "mock-provider".to_string(),
                features: Vec::new(),
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Core),
                rpc_url: Some(rpc_url),
                ..ConnectorConfig::default()
            }],
            ..Config::default()
        };

        SourceChain::from_config(&config, crate::collector::RpcResponseStats::default())?
            .verify_capabilities()
            .await?;
        server_task.abort();
        Ok(())
    }

    #[tokio::test]
    async fn connector_capability_probe_calls_exact_core_and_extended_method_sets() -> Result<()> {
        let (core_url, core_task, core_methods) =
            start_recording_mock_rpc_server(MockRpcProfile::StrictCapabilityParams).await?;
        let core_config = Config {
            connectors: vec![ConnectorConfig {
                name: "core-rpc".to_string(),
                provider: "mock-provider".to_string(),
                features: Vec::new(),
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Core),
                rpc_url: Some(core_url),
                ..ConnectorConfig::default()
            }],
            ..Config::default()
        };
        SourceChain::from_config(&core_config, crate::collector::RpcResponseStats::default())?
            .verify_capabilities()
            .await?;
        let mut core_methods = core_methods.lock().await.clone();
        core_methods.sort();
        assert_eq!(
            core_methods.iter().map(String::as_str).collect::<Vec<_>>(),
            [
                "getEpochInfo",
                "getHealth",
                "getLeaderSchedule",
                "getVoteAccounts"
            ]
        );
        core_task.abort();

        let (extended_url, extended_task, extended_methods) =
            start_recording_mock_rpc_server(MockRpcProfile::StrictCapabilityParams).await?;
        let extended_config = Config {
            connectors: vec![ConnectorConfig {
                name: "extended-rpc".to_string(),
                provider: "mock-provider".to_string(),
                features: Vec::new(),
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
                rpc_url: Some(extended_url),
                ..ConnectorConfig::default()
            }],
            ..Config::default()
        };
        SourceChain::from_config(
            &extended_config,
            crate::collector::RpcResponseStats::default(),
        )?
        .verify_capabilities()
        .await?;
        let mut extended_methods = extended_methods.lock().await.clone();
        extended_methods.sort();
        assert_eq!(
            extended_methods
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [
                "getBlockProduction",
                "getEpochInfo",
                "getHealth",
                "getInflationReward",
                "getLeaderSchedule",
                "getMinimumBalanceForRentExemption",
                "getMultipleAccounts",
                "getRecentPerformanceSamples",
                "getVoteAccounts",
                "minimumLedgerSlot"
            ]
        );
        extended_task.abort();
        Ok(())
    }

    #[tokio::test]
    async fn connector_capability_probe_validates_method_specific_payloads() -> Result<()> {
        let (rpc_url, server_task) =
            start_mock_rpc_server_with_profile(MockRpcProfile::StrictCapabilityParams).await?;
        let config = Config {
            connectors: vec![ConnectorConfig {
                name: "strict-extended-rpc".to_string(),
                provider: "mock-provider".to_string(),
                features: vec![ProviderFeature::HistoricalData],
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
                rpc_url: Some(rpc_url),
                ..ConnectorConfig::default()
            }],
            ..Config::default()
        };

        SourceChain::from_config(&config, crate::collector::RpcResponseStats::default())?
            .verify_capabilities()
            .await?;
        server_task.abort();
        Ok(())
    }

    #[tokio::test]
    async fn connector_capability_probe_checks_every_extended_rpc_method() -> Result<()> {
        for (profile, expected_method) in [
            (MockRpcProfile::NoInflationRewards, "getInflationReward"),
            (MockRpcProfile::NoMinimumLedgerSlot, "minimumLedgerSlot"),
        ] {
            let (rpc_url, server_task) = start_mock_rpc_server_with_profile(profile).await?;
            let config = Config {
                connectors: vec![ConnectorConfig {
                    name: "extended-rpc".to_string(),
                    provider: "mock-provider".to_string(),
                    features: Vec::new(),
                    solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
                    rpc_url: Some(rpc_url),
                    ..ConnectorConfig::default()
                }],
                ..Config::default()
            };

            let Err(error) =
                SourceChain::from_config(&config, crate::collector::RpcResponseStats::default())?
                    .verify_capabilities()
                    .await
            else {
                anyhow::bail!(
                    "{expected_method} must be required by extended Solana RPC compatibility"
                );
            };
            let message = format!("{error:#}");
            assert!(
                message.contains("solana_rpc_compatibility = \"extended\""),
                "{message}"
            );
            assert!(message.contains(expected_method), "{message}");
            assert!(
                !message.contains("provider feature \"historical-data\""),
                "{message}"
            );
            server_task.abort();
        }
        Ok(())
    }

    #[tokio::test]
    async fn connector_capability_probe_rejects_empty_required_results() -> Result<()> {
        for profile in [
            MockRpcProfile::NullPerformanceSamples,
            MockRpcProfile::MissingPerformanceSamplesResult,
        ] {
            let (rpc_url, server_task) = start_mock_rpc_server_with_profile(profile).await?;
            let config = Config {
                connectors: vec![ConnectorConfig {
                    name: "extended-rpc".to_string(),
                    provider: "mock-provider".to_string(),
                    features: Vec::new(),
                    solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
                    rpc_url: Some(rpc_url),
                    ..ConnectorConfig::default()
                }],
                ..Config::default()
            };

            let Err(error) =
                SourceChain::from_config(&config, crate::collector::RpcResponseStats::default())?
                    .verify_capabilities()
                    .await
            else {
                anyhow::bail!("empty required RPC results must fail capability validation");
            };
            let message = format!("{error:#}");
            assert!(message.contains("getRecentPerformanceSamples"), "{message}");
            assert!(message.contains("returned no result"), "{message}");
            server_task.abort();
        }
        Ok(())
    }

    #[tokio::test]
    async fn connector_capability_probe_reports_declared_historical_data_feature() -> Result<()> {
        let (rpc_url, server_task) =
            start_mock_rpc_server_with_profile(MockRpcProfile::NoInflationRewards).await?;
        let config = Config {
            connectors: vec![ConnectorConfig {
                name: "archive-rpc".to_string(),
                provider: "mock-provider".to_string(),
                features: vec![ProviderFeature::HistoricalData],
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
                rpc_url: Some(rpc_url),
                ..ConnectorConfig::default()
            }],
            ..Config::default()
        };
        let Err(error) =
            SourceChain::from_config(&config, crate::collector::RpcResponseStats::default())?
                .verify_capabilities()
                .await
        else {
            anyhow::bail!("missing historical-data RPC method should fail");
        };
        let message = format!("{error:#}");
        assert!(
            message.contains("provider feature \"historical-data\""),
            "{message}"
        );
        assert!(message.contains("getInflationReward"), "{message}");
        server_task.abort();
        Ok(())
    }

    async fn start_mock_rpc_server() -> Result<(String, JoinHandle<()>)> {
        start_mock_rpc_server_with_profile(MockRpcProfile::Default).await
    }

    async fn start_mock_rpc_server_with_profile(
        profile: MockRpcProfile,
    ) -> Result<(String, JoinHandle<()>)> {
        let (rpc_url, task, _) = start_recording_mock_rpc_server(profile).await?;
        Ok((rpc_url, task))
    }

    async fn start_recording_mock_rpc_server(
        profile: MockRpcProfile,
    ) -> Result<(String, JoinHandle<()>, Arc<Mutex<Vec<String>>>)> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .context("failed to bind mock RPC listener")?;
        let address = listener
            .local_addr()
            .context("failed to read mock RPC listener address")?;
        let methods = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/", post(mock_rpc_handler))
            .with_state(MockRpcState {
                profile,
                methods: Arc::clone(&methods),
            });
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        Ok((format!("http://{address}"), task, methods))
    }
    fn capability_probe_params_are_valid(method: &str, params: &[Value]) -> bool {
        let actual = Value::Array(params.to_vec());
        match method {
            "getHealth" | "minimumLedgerSlot" => actual == json!([]),
            "getEpochInfo" => actual == json!([{ "commitment": "confirmed" }]),
            "getVoteAccounts" => {
                actual
                    == json!([{
                        "commitment": "confirmed",
                        "votePubkey": "11111111111111111111111111111111"
                    }])
            }
            "getLeaderSchedule" => actual == json!([810, { "commitment": "confirmed" }]),
            "getRecentPerformanceSamples" => actual == json!([1]),
            "getBlockProduction" => {
                actual
                    == json!([{
                        "commitment": "confirmed",
                        "range": { "firstSlot": 809, "lastSlot": 809 }
                    }])
            }
            "getMultipleAccounts" => {
                actual
                    == json!([
                        ["11111111111111111111111111111111"],
                        {
                            "commitment": "confirmed",
                            "dataSlice": { "offset": 0, "length": 0 },
                            "encoding": "base64"
                        }
                    ])
            }
            "getMinimumBalanceForRentExemption" => {
                actual == json!([0, { "commitment": "confirmed" }])
            }
            "getInflationReward" => {
                actual
                    == json!([
                        ["11111111111111111111111111111111"],
                        { "commitment": "confirmed" }
                    ])
                    || actual
                        == json!([
                            ["11111111111111111111111111111111"],
                            { "commitment": "confirmed", "epoch": 8 }
                        ])
            }
            _ => false,
        }
    }

    async fn mock_rpc_handler(
        State(state): State<MockRpcState>,
        Json(request): Json<Value>,
    ) -> impl IntoResponse {
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        state.methods.lock().await.push(method.to_string());
        let profile = state.profile;
        let params = request
            .get("params")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if matches!(profile, MockRpcProfile::StrictCapabilityParams)
            && !capability_probe_params_are_valid(method, &params)
        {
            return (
                StatusCode::OK,
                Json(rpc_error(-32602, "invalid capability probe params")),
            )
                .into_response();
        }
        if method == "getRecentPerformanceSamples" {
            if matches!(profile, MockRpcProfile::NullPerformanceSamples) {
                return (StatusCode::OK, Json(rpc_success(Value::Null))).into_response();
            }
            if matches!(profile, MockRpcProfile::MissingPerformanceSamplesResult) {
                return (StatusCode::OK, Json(json!({ "jsonrpc": "2.0", "id": 1 })))
                    .into_response();
            }
        }

        match method {
            "getHealth" => {
                let health = if matches!(profile, MockRpcProfile::Unhealthy) {
                    "unhealthy"
                } else {
                    "ok"
                };
                (StatusCode::OK, Json(rpc_success(json!(health)))).into_response()
            }
            "getEpochInfo" => {
                if matches!(profile, MockRpcProfile::NoEpochInfo) {
                    return (StatusCode::OK, Json(rpc_error(-32601, "method not found")))
                        .into_response();
                }

                (
                    StatusCode::OK,
                    Json(rpc_success(json!({
                        "epoch": 9,
                        "slotIndex": 10,
                        "slotsInEpoch": 100,
                        "absoluteSlot": 810
                    }))),
                )
                    .into_response()
            }
            "getRecentPerformanceSamples" => {
                if matches!(profile, MockRpcProfile::NoPerformanceSamples) {
                    return (
                        StatusCode::OK,
                        Json(rpc_error(
                            -32601,
                            "method not found at /?api-key=do-not-expose-capability-probe-key",
                        )),
                    )
                        .into_response();
                }
                (
                    StatusCode::OK,
                    Json(rpc_success(json!([{
                        "slot": 810,
                        "numTransactions": 1000,
                        "numSlots": 100,
                        "samplePeriodSecs": 40,
                        "numNonVoteTransactions": 100
                    }]))),
                )
                    .into_response()
            }
            "getVoteAccounts" => (
                StatusCode::OK,
                Json(rpc_success(json!({
                    "current": [{
                        "votePubkey": "vote-1",
                        "nodePubkey": "id-1",
                        "activatedStake": 1_500_000_000u64,
                        "commission": 5,
                        "epochCredits": [[8, 20, 10]],
                        "lastVote": 809,
                        "rootSlot": 808
                    }],
                    "delinquent": []
                }))),
            )
                .into_response(),
            "getBlockProduction" => {
                let first_slot = params
                    .first()
                    .and_then(|config| config.get("range"))
                    .and_then(|range| range.get("firstSlot"))
                    .and_then(Value::as_u64);
                let last_slot = params
                    .first()
                    .and_then(|config| config.get("range"))
                    .and_then(|range| range.get("lastSlot"))
                    .and_then(Value::as_u64);

                if first_slot.is_none()
                    && matches!(profile, MockRpcProfile::NoCurrentBlockProduction)
                {
                    return (StatusCode::OK, Json(rpc_error(-32601, "method not found")))
                        .into_response();
                }
                if let (Some(first_slot), Some(last_slot)) = (first_slot, last_slot) {
                    if last_slot.saturating_sub(first_slot).saturating_add(1)
                        > MAX_BLOCK_PRODUCTION_RANGE_SLOTS
                    {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(rpc_error(
                                -32600,
                                "Requested slot range too large. Maximum range is 5000 slots.",
                            )),
                        )
                            .into_response();
                    }
                }

                let by_identity = match first_slot {
                    Some(700) => json!({ "id-1": [4, 3] }),
                    Some(5_700) => json!({ "id-1": [2, 2] }),
                    Some(10_700) => json!({ "id-1": [1, 1] }),
                    Some(_) => json!({ "id-1": [5, 4] }),
                    None => json!({ "id-1": [5, 4] }),
                };

                (
                    StatusCode::OK,
                    Json(rpc_success(
                        json!({ "value": { "byIdentity": by_identity } }),
                    )),
                )
                    .into_response()
            }
            "getSlotLeader" => {
                if matches!(profile, MockRpcProfile::NoSlotLeader) {
                    return (StatusCode::OK, Json(rpc_error(-32601, "method not found")))
                        .into_response();
                }
                (StatusCode::OK, Json(rpc_success(json!("id-1")))).into_response()
            }
            "getLeaderSchedule" => {
                if matches!(profile, MockRpcProfile::NoLeaderSchedule) {
                    return (StatusCode::OK, Json(rpc_error(-32601, "method not found")))
                        .into_response();
                }

                let slot = params.first().and_then(Value::as_u64).unwrap_or_default();
                let schedule = if slot == 810 {
                    json!({ "id-1": [0, 2, 5, 10] })
                } else {
                    json!({ "id-1": [1, 50] })
                };
                (StatusCode::OK, Json(rpc_success(schedule))).into_response()
            }
            "getMultipleAccounts" => {
                let vote_account_space = if matches!(profile, MockRpcProfile::NoAccountSpace) {
                    Value::Null
                } else {
                    json!(3762)
                };
                (
                    StatusCode::OK,
                    Json(rpc_success(json!({
                        "context": { "slot": 810 },
                        "value": [
                            {
                                "lamports": 3_000_000_000u64,
                                "owner": "11111111111111111111111111111111",
                                "data": ["", "base64"],
                                "executable": false,
                                "rentEpoch": 0,
                                "space": 0
                            },
                            {
                                "lamports": 2_500_000_000u64,
                                "owner": "Vote111111111111111111111111111111111111111",
                                "data": ["", "base64"],
                                "executable": false,
                                "rentEpoch": 0,
                                "space": vote_account_space
                            }
                        ]
                    }))),
                )
                    .into_response()
            }
            "getMinimumBalanceForRentExemption" => {
                (StatusCode::OK, Json(rpc_success(json!(900_000_000u64)))).into_response()
            }
            "minimumLedgerSlot" => {
                if matches!(profile, MockRpcProfile::NoMinimumLedgerSlot) {
                    return (StatusCode::OK, Json(rpc_error(-32601, "method not found")))
                        .into_response();
                }
                (StatusCode::OK, Json(rpc_success(json!(600)))).into_response()
            }
            "getInflationReward" => {
                if matches!(profile, MockRpcProfile::NoInflationRewards) {
                    return (StatusCode::OK, Json(rpc_error(-32601, "method not found")))
                        .into_response();
                }
                let rewards = params
                    .first()
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|_| json!({ "amount": 12_000_000_000i64, "epoch": 8 }))
                    .collect::<Vec<_>>();
                (StatusCode::OK, Json(rpc_success(json!(rewards)))).into_response()
            }
            _ => (StatusCode::OK, Json(rpc_error(-32601, "method not found"))).into_response(),
        }
    }

    fn rpc_success(result: Value) -> Value {
        json!({
            "jsonrpc": "2.0",
            "result": result,
            "id": 1
        })
    }

    fn rpc_error(code: i64, message: &str) -> Value {
        json!({
            "jsonrpc": "2.0",
            "error": {
                "code": code,
                "message": message
            },
            "id": 1
        })
    }

    fn modes_for(connectors: &[&str], economics_enabled: bool) -> ModesConfig {
        let route = || connectors.iter().map(|name| (*name).to_string()).collect();
        ModesConfig {
            health: HealthModeConfig {
                commands: HealthCommands {
                    rpc_health: route(),
                    ws_health: vec![],
                },
            },
            consensus: ConsensusModeConfig {
                commands: ConsensusCommands {
                    epoch_info: route(),
                    vote_accounts: route(),
                    recent_performance_samples: route(),
                },
            },
            leadership: LeadershipModeConfig {
                commands: LeadershipCommands {
                    current_block_production: route(),
                    current_leader_schedule: route(),
                    next_leader_schedule: route(),
                },
            },
            economics: EconomicsModeConfig {
                enabled: economics_enabled,
                validator: economics_enabled.then(|| WatchedValidatorConfig {
                    identity: Some("id-1".to_string()),
                    vote_account: Some("vote-1".to_string()),
                    labels: vec![],
                }),
                commands: EconomicsCommands {
                    account_balances: economics_enabled.then(route).unwrap_or_default(),
                    inflation_rewards: economics_enabled.then(route).unwrap_or_default(),
                    reward_epoch_block_production: economics_enabled
                        .then(route)
                        .unwrap_or_default(),
                },
            },
        }
    }

    #[test]
    fn aggregate_records_emits_zeroed_builtin_scopes_for_empty_clusters() -> Result<()> {
        let snapshot = aggregate_records("localnet".to_string(), 0, 42, &[]);

        let all_network = snapshot
            .scopes
            .get("scope:all-network")
            .ok_or_else(|| anyhow!("missing all-network scope"))?;
        let network = snapshot
            .scopes
            .get("scope:network")
            .ok_or_else(|| anyhow!("missing network scope"))?;
        let watched = snapshot
            .scopes
            .get("scope:watched")
            .ok_or_else(|| anyhow!("missing watched scope"))?;

        assert_eq!(snapshot.network, "localnet");
        assert_eq!(snapshot.slot, 42);
        assert_eq!(all_network, &ScopeMetrics::default());
        assert_eq!(network, &ScopeMetrics::default());
        assert_eq!(watched, &ScopeMetrics::default());
        Ok(())
    }
}
