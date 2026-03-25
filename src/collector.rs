use crate::{
    config::Config,
    labels::{LabelMatcher, LABEL_SCOPE_ALL_NETWORK, LABEL_SCOPE_NETWORK, LABEL_SCOPE_WATCHED},
};
use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};
use tracing::warn;

#[derive(Clone)]
pub struct RpcClient {
    http: Client,
    url: String,
    commitment: String,
}

impl RpcClient {
    pub fn new(url: String, timeout: Duration, commitment: String) -> Result<Self> {
        let http = Client::builder()
            .timeout(timeout)
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self {
            http,
            url,
            commitment,
        })
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

    async fn get_block_production(
        &self,
        first_slot: u64,
        last_slot: u64,
    ) -> Result<BlockProduction> {
        let response: RpcValueResponse<BlockProduction> = self
            .call(
                "getBlockProduction",
                [GetBlockProductionConfig {
                    commitment: &self.commitment,
                    range: SlotRange {
                        first_slot,
                        last_slot,
                    },
                }],
            )
            .await?;

        Ok(response.value)
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

    async fn get_inflation_rewards(
        &self,
        addresses: &[String],
        epoch: u64,
    ) -> Result<HashMap<String, RewardSample>> {
        if addresses.is_empty() {
            return Ok(HashMap::new());
        }

        let rewards: Vec<Option<InflationRewardValue>> = self
            .call(
                "getInflationReward",
                (
                    addresses,
                    GetInflationRewardConfig {
                        commitment: &self.commitment,
                        epoch,
                    },
                ),
            )
            .await?;

        if rewards.len() != addresses.len() {
            bail!(
                "getInflationReward returned {} rewards for {} addresses",
                rewards.len(),
                addresses.len()
            );
        }

        let mut by_address = HashMap::with_capacity(addresses.len());
        for (address, reward) in addresses.iter().zip(rewards) {
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

        Ok(by_address)
    }

    pub(crate) async fn ensure_healthy(&self) -> Result<()> {
        let health = self
            .get_health()
            .await
            .with_context(|| format!("RPC endpoint {} failed getHealth", self.url))?;

        if health != "ok" {
            bail!(
                "RPC endpoint {} returned unexpected getHealth result {:?}",
                self.url,
                health
            );
        }

        Ok(())
    }

    async fn call<T, P>(&self, method: &'static str, params: P) -> Result<T>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        self.call_optional(method, params)
            .await?
            .with_context(|| format!("{} returned no result", method))
    }

    async fn call_optional<T, P>(&self, method: &'static str, params: P) -> Result<Option<T>>
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
            .with_context(|| format!("{} request failed", method))?
            .error_for_status()
            .with_context(|| format!("{} returned non-success HTTP status", method))?;

        let envelope: JsonRpcEnvelope<T> = response
            .json()
            .await
            .with_context(|| format!("{} returned an invalid JSON-RPC payload", method))?;

        if let Some(error) = envelope.error {
            bail!(
                "{} failed with code {}: {}",
                method,
                error.code,
                error.message
            );
        }

        Ok(envelope.result)
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

#[derive(Clone, Debug, PartialEq)]
pub struct WatchedValidatorMetrics {
    pub identity: String,
    pub vote_account: String,
    pub status: ValidatorStatus,
    pub scopes: Vec<String>,
    pub activated_stake_lamports: u64,
    pub latest_epoch_vote_credits: u64,
    pub vote_efficiency_pct: f64,
    pub last_vote_slot_lag: u64,
    pub root_slot_lag: u64,
    pub commission_pct: u8,
    pub latest_epoch_reward_lamports: u64,
    pub latest_epoch_rewarded_blocks_produced: u64,
    pub blocks_produced: u64,
    pub blocks_missed: u64,
    pub block_production_rate: f64,
    pub future_leader_slots: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
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
    ) -> f64 {
        let Some(credits) = self.matching_epoch_credits(sample_epoch) else {
            return 0.0;
        };
        let Some(ideal) = ideal_epoch_credits.filter(|ideal| *ideal > 0) else {
            return 0.0;
        };

        ratio(credits, ideal)
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
    pub latest_epoch_vote_credits: u64,
    pub latest_epoch_reward_lamports: u64,
    pub latest_epoch_rewarded_blocks_produced: u64,
    pub vote_efficiency_pct: f64,
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
    pub vote_credits_sample_epoch: Option<u64>,
    pub reward_sample_epoch: Option<u64>,
    pub scopes: BTreeMap<String, ScopeMetrics>,
    pub watched_validators: Vec<WatchedValidatorMetrics>,
}

#[derive(Default)]
struct ScopeAccumulator {
    current_count: u64,
    delinquent_count: u64,
    current_stake_lamports: u64,
    delinquent_stake_lamports: u64,
    latest_epoch_vote_credits: u64,
    latest_epoch_reward_lamports: u64,
    latest_epoch_rewarded_blocks_produced: u64,
    weighted_vote_efficiency_sum: f64,
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
            self.latest_epoch_vote_credits += credits;
            self.weighted_vote_efficiency_sum +=
                weight * record.vote_efficiency_pct(ideal_epoch_credits, sample_epoch);
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
            vote_efficiency_pct: self.weighted_vote_efficiency_sum / weight_sum,
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

pub async fn collect_snapshot(client: &RpcClient, config: &Config) -> Result<Snapshot> {
    let (epoch_info_result, vote_accounts_result) = tokio::join!(
        async { client.get_epoch_info().await.context("getEpochInfo failed") },
        async {
            client
                .get_vote_accounts(config.delinquent_slot_distance)
                .await
                .context("getVoteAccounts failed")
        },
    );

    let epoch_info = epoch_info_result?;
    let vote_accounts = vote_accounts_result?;

    let current_slot = epoch_info.absolute_slot;
    let epoch_start_slot = current_slot.saturating_sub(epoch_info.slot_index);
    let next_epoch_start_slot = epoch_start_slot + epoch_info.slots_in_epoch;
    let reward_epoch = epoch_info.epoch.checked_sub(1);
    let reward_epoch_start_slot = epoch_start_slot.saturating_sub(epoch_info.slots_in_epoch);
    let reward_epoch_end_slot = epoch_start_slot.saturating_sub(1);
    let reward_vote_accounts = vote_accounts
        .current
        .iter()
        .chain(vote_accounts.delinquent.iter())
        .map(|info| info.vote_pubkey.clone())
        .collect::<Vec<_>>();

    let (
        block_production_result,
        current_schedule_result,
        next_schedule_result,
        reward_epoch_block_production_result,
        inflation_rewards_result,
    ) = tokio::join!(
        async {
            client
                .get_block_production(epoch_start_slot, current_slot)
                .await
                .context("getBlockProduction failed")
        },
        async {
            client
                .get_leader_schedule(current_slot)
                .await
                .context("getLeaderSchedule for current epoch failed")
        },
        async {
            client
                .get_leader_schedule(next_epoch_start_slot)
                .await
                .context("getLeaderSchedule for next epoch failed")
        },
        async {
            match reward_epoch {
                Some(_) => client
                    .get_block_production(reward_epoch_start_slot, reward_epoch_end_slot)
                    .await
                    .context("getBlockProduction for reward epoch failed"),
                None => Ok(BlockProduction::default()),
            }
        },
        async {
            match reward_epoch {
                Some(epoch) => client
                    .get_inflation_rewards(&reward_vote_accounts, epoch)
                    .await
                    .context("getInflationReward failed"),
                None => Ok(HashMap::new()),
            }
        },
    );

    let block_production = optional_rpc_or_default(
        block_production_result,
        &config.validator_connection.rpc_url,
        "getBlockProduction",
    );

    let current_schedule = optional_rpc_or_default(
        current_schedule_result,
        &config.validator_connection.rpc_url,
        "getLeaderSchedule(current epoch)",
    );

    let next_schedule = optional_rpc_or_default(
        next_schedule_result,
        &config.validator_connection.rpc_url,
        "getLeaderSchedule(next epoch)",
    );
    let reward_epoch_block_production = optional_rpc_or_default(
        reward_epoch_block_production_result,
        &config.validator_connection.rpc_url,
        "getBlockProduction(reward epoch)",
    );
    let inflation_rewards = optional_rpc_or_default(
        inflation_rewards_result,
        &config.validator_connection.rpc_url,
        "getInflationReward",
    );

    let matcher = LabelMatcher::from_config(config);
    let build_context = RecordBuildContext {
        matcher: &matcher,
        current_slot,
        epoch_start_slot,
        by_identity: &block_production.by_identity,
        reward_epoch_by_identity: &reward_epoch_block_production.by_identity,
        current_schedule: current_schedule.as_ref(),
        next_schedule: next_schedule.as_ref(),
        latest_epoch_rewards: &inflation_rewards,
    };
    let mut records =
        Vec::with_capacity(vote_accounts.current.len() + vote_accounts.delinquent.len());

    for info in vote_accounts.current {
        records.push(build_record(info, ValidatorStatus::Current, &build_context));
    }

    for info in vote_accounts.delinquent {
        records.push(build_record(
            info,
            ValidatorStatus::Delinquent,
            &build_context,
        ));
    }

    Ok(aggregate_records(
        config.network.clone(),
        epoch_info.epoch,
        current_slot,
        &records,
    ))
}

struct RecordBuildContext<'a> {
    matcher: &'a LabelMatcher,
    current_slot: u64,
    epoch_start_slot: u64,
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
        .map_or((0, 0), |entry| entry);
    let reward_epoch_blocks_produced = context
        .reward_epoch_by_identity
        .get(&info.node_pubkey)
        .copied()
        .map_or(0, |(_, produced)| produced);

    let future_current_epoch = context
        .current_schedule
        .and_then(|schedule| schedule.get(&info.node_pubkey))
        .map_or(0, |slots| {
            slots
                .iter()
                .filter(|relative_slot| {
                    context.epoch_start_slot + **relative_slot >= context.current_slot
                })
                .count() as u64
        });

    let future_next_epoch = context
        .next_schedule
        .and_then(|schedule| schedule.get(&info.node_pubkey))
        .map_or(0, |slots| slots.len() as u64);

    let latest_epoch_credits = info
        .epoch_credits
        .iter()
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
        future_leader_slots: future_current_epoch + future_next_epoch,
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
        vote_credits_sample_epoch: sample_epoch,
        reward_sample_epoch,
        scopes: scopes
            .into_iter()
            .map(|(scope, metrics)| (scope, metrics.finish()))
            .collect(),
        watched_validators,
    }
}

fn build_watched_validator_metrics(
    record: &ValidatorRecord,
    current_slot: u64,
    ideal_epoch_credits: Option<u64>,
    sample_epoch: Option<u64>,
    reward_sample_epoch: Option<u64>,
) -> WatchedValidatorMetrics {
    let latest_epoch_vote_credits = record
        .matching_epoch_credits(sample_epoch)
        .unwrap_or_default();
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
    }
}

fn optional_rpc_or_default<T>(result: Result<T>, rpc_url: &str, component: &str) -> T
where
    T: Default,
{
    match result {
        Ok(value) => value,
        Err(err) => {
            warn!(
                rpc_url = %rpc_url,
                component,
                error = ?err,
                "optional RPC data unavailable; continuing with default values"
            );
            T::default()
        }
    }
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
struct GetBlockProductionConfig<'a> {
    commitment: &'a str,
    range: SlotRange,
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
struct JsonRpcEnvelope<T> {
    result: Option<T>,
    error: Option<JsonRpcError>,
}

#[derive(Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcValueResponse<T> {
    value: T,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EpochInfo {
    epoch: u64,
    slot_index: u64,
    slots_in_epoch: u64,
    absolute_slot: u64,
}

#[derive(Deserialize)]
struct VoteAccounts {
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

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlockProduction {
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
        aggregate_records, build_record, build_watched_validator_metrics, optional_rpc_or_default,
        ratio, EpochCreditsSample, RecordBuildContext, RewardSample, ScopeMetrics, ValidatorRecord,
        ValidatorStatus, VoteAccountInfo, WatchedValidatorMetrics,
    };
    use crate::{
        config::{
            Config, TransportMode, ValidatorConnectionConfig, ValidatorMode, WatchedValidatorConfig,
        },
        labels::LabelMatcher,
    };
    use anyhow::{anyhow, Result};
    use std::collections::HashMap;

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
                latest_epoch_vote_credits: 10,
                latest_epoch_reward_lamports: 9_000_000_000,
                latest_epoch_rewarded_blocks_produced: 3,
                vote_efficiency_pct: 100.0,
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
        assert_eq!(all_network.latest_epoch_vote_credits, 15);
        assert!(all_network.vote_efficiency_pct > 80.0);
        assert_eq!(
            snapshot.watched_validators,
            vec![WatchedValidatorMetrics {
                identity: "id-1".to_string(),
                vote_account: "vote-1".to_string(),
                status: ValidatorStatus::Current,
                scopes: vec!["scope:watched".to_string(), "scope:all-network".to_string()],
                activated_stake_lamports: 10,
                latest_epoch_vote_credits: 10,
                latest_epoch_reward_lamports: 9_000_000_000,
                latest_epoch_rewarded_blocks_produced: 3,
                vote_efficiency_pct: 100.0,
                last_vote_slot_lag: 5,
                root_slot_lag: 6,
                commission_pct: 5,
                blocks_produced: 3,
                blocks_missed: 1,
                block_production_rate: 75.0,
                future_leader_slots: 2,
            }]
        );

        Ok(())
    }

    #[test]
    fn watched_validator_metrics_use_zero_efficiency_without_matching_epoch() {
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
                latest_epoch_vote_credits: 0,
                latest_epoch_reward_lamports: 0,
                latest_epoch_rewarded_blocks_produced: 0,
                vote_efficiency_pct: 0.0,
                last_vote_slot_lag: 2,
                root_slot_lag: 3,
                commission_pct: 7,
                blocks_produced: 0,
                blocks_missed: 0,
                block_production_rate: 0.0,
                future_leader_slots: 3,
            }
        );
    }

    #[test]
    fn ratio_returns_zero_for_empty_denominator() {
        assert_eq!(ratio(0, 0), 0.0);
        assert_eq!(ratio(5, 0), 0.0);
        assert_eq!(ratio(3, 4), 75.0);
    }

    #[test]
    fn optional_rpc_or_default_preserves_successful_values() {
        let value = optional_rpc_or_default(Ok(vec![1_u8, 2_u8]), "https://rpc.invalid", "test");
        assert_eq!(value, vec![1_u8, 2_u8]);
    }

    #[test]
    fn optional_rpc_or_default_falls_back_to_default_on_error() {
        let value: Vec<u8> =
            optional_rpc_or_default(Err(anyhow!("boom")), "https://rpc.invalid", "test");
        assert!(value.is_empty());
    }

    #[test]
    fn build_record_uses_block_production_schedules_and_labels() {
        let config = Config {
            network: "devnet".to_string(),
            validator_connection: ValidatorConnectionConfig {
                rpc_url: "https://rpc.example".to_string(),
                transport_mode: TransportMode::PollHttpAndWs,
                ws_url: Some("wss://rpc.example".to_string()),
                ..ValidatorConnectionConfig::default()
            },
            validator_mode: ValidatorMode::Rpc,
            validator_websocket_enabled: true,
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
            current_slot: 110,
            epoch_start_slot: 100,
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
                epoch_credits: vec![(8, 20, 12), (9, 35, 20)],
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
            validator_connection: ValidatorConnectionConfig {
                rpc_url: "https://rpc.example".to_string(),
                transport_mode: TransportMode::PollHttp,
                ..ValidatorConnectionConfig::default()
            },
            validator_mode: ValidatorMode::Observer,
            ..Config::default()
        };
        let matcher = LabelMatcher::from_config(&config);
        let by_identity = HashMap::new();
        let reward_epoch_by_identity = HashMap::new();
        let latest_epoch_rewards = HashMap::new();

        let context = RecordBuildContext {
            matcher: &matcher,
            current_slot: 50,
            epoch_start_slot: 40,
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
        assert_eq!(record.latest_epoch_credits, None);
        assert_eq!(record.latest_epoch_reward, None);
        assert!(record.scopes.contains(&"scope:network".to_string()));
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
