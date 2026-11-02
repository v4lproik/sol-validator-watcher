pub mod command;

use crate::{
    config::Config,
    labels::{LabelMatcher, LABEL_SCOPE_ALL_NETWORK, LABEL_SCOPE_NETWORK, LABEL_SCOPE_WATCHED},
    mode::{CommandId, ModeId},
    source::{SourceChain, SourceSelection},
};
use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
    time::Duration,
};

const MAX_BLOCK_PRODUCTION_RANGE_SLOTS: u64 = 5_000;
const MAX_INFLATION_REWARD_ADDRESSES_PER_REQUEST: usize = 100;

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
            .with_context(|| format!("{} request failed", method))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .with_context(|| format!("{} response body read failed", method))?;

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
            bail!(
                "{} failed with code {}: {}",
                method,
                error.code,
                error.message
            );
        }

        if !status.is_success() {
            bail!("{} returned non-success HTTP status {}", method, status);
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
    pub data_sources: Vec<SnapshotDataSource>,
    pub mode_statuses: Vec<ModeStatus>,
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
    pub precedence: usize,
}

impl From<SourceSelection> for SnapshotDataSource {
    fn from(source: SourceSelection) -> Self {
        Self {
            mode: source.mode,
            command: source.command,
            connector: source.connector,
            provider: source.provider,
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
    let current_slot = epoch_info.value.absolute_slot;
    let epoch_start_slot = current_slot.saturating_sub(epoch_info.value.slot_index);

    let Some(reward_epoch) = epoch_info.value.epoch.checked_sub(1) else {
        return Ok(());
    };
    let reward_addresses = economics_preflight_reward_addresses(config, &vote_accounts.value);
    sources
        .required(&command::GetInflationRewards {
            addresses: &reward_addresses,
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
    let (epoch_info, vote_accounts) = tokio::join!(
        sources.required(&epoch_command),
        sources.required(&vote_accounts_command),
    );
    let epoch_info = epoch_info?;
    let vote_accounts = vote_accounts?;
    let mut data_sources = vec![
        epoch_info.source.clone().into(),
        vote_accounts.source.clone().into(),
    ];

    let current_slot = epoch_info.value.absolute_slot;
    let epoch_start_slot = current_slot.saturating_sub(epoch_info.value.slot_index);
    let next_epoch_start_slot = epoch_start_slot + epoch_info.value.slots_in_epoch;
    let reward_epoch = epoch_info.value.epoch.checked_sub(1);
    let economics_enabled = config.modes.economics.enabled;
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
            let (Some(epoch), Some(addresses)) = (reward_epoch, reward_vote_accounts.as_deref())
            else {
                return Ok::<_, anyhow::Error>((None, None));
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
            Ok((Some(inflation_rewards?), Some(block_production?)))
        },
    );

    let (inflation_rewards, reward_epoch_block_production) = economics_outputs?;
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
        epoch_start_slot,
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
    snapshot.data_sources = data_sources;
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
        .unwrap_or((0, 0));
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
        data_sources: vec![],
        mode_statuses: vec![],
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

fn economics_preflight_reward_addresses(
    config: &Config,
    vote_accounts: &VoteAccounts,
) -> Vec<String> {
    if let Some(vote_account) = config
        .modes
        .economics
        .validator
        .as_ref()
        .and_then(|validator| validator.vote_account.clone())
    {
        return vec![vote_account];
    }

    vote_accounts
        .current
        .iter()
        .chain(vote_accounts.delinquent.iter())
        .take(1)
        .map(|info| info.vote_pubkey.clone())
        .collect()
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
        aggregate_records, build_record, build_watched_validator_metrics, collect_snapshot,
        economics_preflight_reward_addresses, ensure_economics_supported, ratio,
        reward_epoch_block_range, BlockProduction, EpochCreditsSample, GetBlockProductionConfig,
        RecordBuildContext, RewardSample, RpcClient, RpcValueResponse, ScopeMetrics, SlotRange,
        ValidatorRecord, ValidatorStatus, VoteAccountInfo, VoteAccounts, WatchedValidatorMetrics,
        MAX_BLOCK_PRODUCTION_RANGE_SLOTS,
    };
    use crate::{
        config::{
            Config, ConnectorConfig, ConsensusCommands, ConsensusModeConfig, EconomicsCommands,
            EconomicsModeConfig, HealthCommands, HealthModeConfig, LeadershipCommands,
            LeadershipModeConfig, ModesConfig, ProviderFeature, WatchedValidatorConfig,
        },
        labels::LabelMatcher,
        mode::{CommandId, ModeId},
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
    use std::{collections::HashMap, time::Duration};
    use tokio::{net::TcpListener, task::JoinHandle};

    #[derive(Clone, Copy)]
    enum MockRpcProfile {
        Default,
        NoCurrentBlockProduction,
        NoEpochInfo,
        NoLeaderSchedule,
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

    #[tokio::test]
    async fn get_block_production_for_range_batches_large_requests() -> Result<()> {
        let (rpc_url, server_task) = start_mock_rpc_server().await?;
        let client = RpcClient::new(rpc_url, Duration::from_secs(5), "confirmed".to_string())?;

        let block_production = client.get_block_production_for_range(700, 10_799).await?;

        server_task.abort();

        assert_eq!(block_production.by_identity.get("id-1"), Some(&(7, 6)));

        Ok(())
    }

    #[tokio::test]
    async fn call_optional_surfaces_json_rpc_errors_from_http_400() -> Result<()> {
        let (rpc_url, server_task) = start_mock_rpc_server().await?;
        let client = RpcClient::new(rpc_url, Duration::from_secs(5), "confirmed".to_string())?;

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
            .contains("getBlockProduction failed with code -32600"));

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
                features: vec![ProviderFeature::Rpc, ProviderFeature::HistoricalRpc],
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
        )?;

        client.ensure_healthy().await?;
        let sources = SourceChain::from_config(&config)?;
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
                    features: vec![ProviderFeature::Rpc],
                    rpc_url: Some(primary_rpc_url),
                    rpc_timeout_sec: 5,
                    ..ConnectorConfig::default()
                },
                ConnectorConfig {
                    name: "helius-fallback".to_string(),
                    provider: "helius".to_string(),
                    features: vec![ProviderFeature::Rpc],
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
        let sources = SourceChain::from_config(&config)?;
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
                    features: vec![ProviderFeature::Rpc],
                    rpc_url: Some(primary_rpc_url),
                    rpc_timeout_sec: 5,
                    ..ConnectorConfig::default()
                },
                ConnectorConfig {
                    name: "helius-fallback".to_string(),
                    provider: "helius".to_string(),
                    features: vec![ProviderFeature::Rpc],
                    rpc_url: Some(fallback_rpc_url),
                    rpc_timeout_sec: 5,
                    ..ConnectorConfig::default()
                },
            ],
            modes: modes_for(&["local-validator", "helius-fallback"], false),
            ..Config::default()
        };
        let sources = SourceChain::from_config(&config)?;
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
                    features: vec![ProviderFeature::Rpc],
                    rpc_url: Some(first_rpc_url),
                    rpc_timeout_sec: 5,
                    ..ConnectorConfig::default()
                },
                ConnectorConfig {
                    name: "second".to_string(),
                    provider: "helius".to_string(),
                    features: vec![ProviderFeature::Rpc],
                    rpc_url: Some(second_rpc_url),
                    rpc_timeout_sec: 5,
                    ..ConnectorConfig::default()
                },
            ],
            modes,
            ..Config::default()
        };
        let sources = SourceChain::from_config(&config)?;
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
                    features: vec![ProviderFeature::Rpc],
                    rpc_url: Some(first_rpc_url),
                    rpc_timeout_sec: 5,
                    ..ConnectorConfig::default()
                },
                ConnectorConfig {
                    name: "second".to_string(),
                    provider: "helius".to_string(),
                    features: vec![ProviderFeature::Rpc],
                    rpc_url: Some(second_rpc_url),
                    rpc_timeout_sec: 5,
                    ..ConnectorConfig::default()
                },
            ],
            modes: modes_for(&["first", "second"], false),
            ..Config::default()
        };
        let sources = SourceChain::from_config(&config)?;

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
                features: vec![ProviderFeature::Rpc, ProviderFeature::HistoricalRpc],
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
        let sources = SourceChain::from_config(&config)?;

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
        let client = RpcClient::new(rpc_url, Duration::from_secs(5), "confirmed".to_string())?;
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
                features: vec![ProviderFeature::Rpc, ProviderFeature::Websocket],
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
            connectors: vec![ConnectorConfig {
                name: "primary".to_string(),
                provider: "solana-validator".to_string(),
                features: vec![ProviderFeature::Rpc],
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
    fn economics_preflight_prefers_configured_vote_account() {
        let config = Config {
            modes: ModesConfig {
                economics: EconomicsModeConfig {
                    enabled: true,
                    validator: Some(crate::config::WatchedValidatorConfig {
                        identity: Some("id-1".to_string()),
                        vote_account: Some("vote-configured".to_string()),
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
                vote_pubkey: "vote-cluster".to_string(),
                node_pubkey: "id-cluster".to_string(),
                activated_stake: 0,
                commission: 0,
                epoch_credits: vec![],
                last_vote: 0,
                root_slot: 0,
            }],
            delinquent: vec![],
        };

        assert_eq!(
            economics_preflight_reward_addresses(&config, &vote_accounts),
            vec!["vote-configured".to_string()]
        );
    }

    #[test]
    fn economics_preflight_falls_back_to_cluster_vote_account() {
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
                vote_pubkey: "vote-cluster".to_string(),
                node_pubkey: "id-cluster".to_string(),
                activated_stake: 0,
                commission: 0,
                epoch_credits: vec![],
                last_vote: 0,
                root_slot: 0,
            }],
        };

        assert_eq!(
            economics_preflight_reward_addresses(&config, &vote_accounts),
            vec!["vote-cluster".to_string()]
        );
    }

    async fn start_mock_rpc_server() -> Result<(String, JoinHandle<()>)> {
        start_mock_rpc_server_with_profile(MockRpcProfile::Default).await
    }

    async fn start_mock_rpc_server_with_profile(
        profile: MockRpcProfile,
    ) -> Result<(String, JoinHandle<()>)> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .context("failed to bind mock RPC listener")?;
        let address = listener
            .local_addr()
            .context("failed to read mock RPC listener address")?;
        let app = Router::new()
            .route("/", post(mock_rpc_handler))
            .with_state(profile);
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        Ok((format!("http://{address}"), task))
    }

    async fn mock_rpc_handler(
        State(profile): State<MockRpcProfile>,
        Json(request): Json<Value>,
    ) -> impl IntoResponse {
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let params = request
            .get("params")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        match method {
            "getHealth" => (StatusCode::OK, Json(rpc_success(json!("ok")))).into_response(),
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
            "minimumLedgerSlot" => (StatusCode::OK, Json(rpc_success(json!(600)))).into_response(),
            "getInflationReward" => {
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
                validator: None,
                commands: EconomicsCommands {
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
