use crate::{
    config::Config,
    labels::{LabelMatcher, LABEL_SCOPE_WATCHED},
};
use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};

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
            json!([CommitmentOnly {
                commitment: &self.commitment,
            }]),
        )
        .await
    }

    async fn get_vote_accounts(
        &self,
        delinquent_slot_distance: Option<u64>,
    ) -> Result<VoteAccounts> {
        self.call(
            "getVoteAccounts",
            json!([GetVoteAccountsConfig {
                commitment: &self.commitment,
                keep_unstaked_delinquents: true,
                delinquent_slot_distance,
            }]),
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
                json!([GetBlockProductionConfig {
                    commitment: &self.commitment,
                    range: SlotRange {
                        first_slot,
                        last_slot,
                    },
                }]),
            )
            .await?;

        Ok(response.value)
    }

    async fn get_leader_schedule(&self, slot: u64) -> Result<Option<HashMap<String, Vec<u64>>>> {
        self.call(
            "getLeaderSchedule",
            json!([
                slot,
                CommitmentOnly {
                    commitment: &self.commitment,
                }
            ]),
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn probe(&self) -> Result<()> {
        self.get_epoch_info().await.map(|_| ())
    }

    async fn call<T>(&self, method: &str, params: Value) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let response = self
            .http
            .post(&self.url)
            .json(&json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": method,
                "params": params,
            }))
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

        envelope
            .result
            .with_context(|| format!("{} returned no result", method))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidatorStatus {
    Current,
    Delinquent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EpochCreditsSample {
    pub epoch: u64,
    pub earned: u64,
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
    pub current_epoch_leader_slots: u64,
    pub current_epoch_blocks_produced: u64,
    pub future_leader_slots: u64,
    pub scopes: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScopeMetrics {
    pub current_count: u64,
    pub delinquent_count: u64,
    pub current_stake_lamports: u64,
    pub delinquent_stake_lamports: u64,
    pub latest_epoch_vote_credits: u64,
    pub vote_efficiency_pct: f64,
    pub last_vote_slot_lag: f64,
    pub root_slot_lag: f64,
    pub average_commission_pct: f64,
    pub blocks_produced: u64,
    pub blocks_missed: u64,
    pub block_production_rate: f64,
    pub future_leader_slots: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub network: String,
    pub epoch: u64,
    pub slot: u64,
    pub vote_credits_sample_epoch: Option<u64>,
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
            weight * current_slot.saturating_sub(record.last_vote) as f64;
        self.weighted_root_slot_lag_sum +=
            weight * current_slot.saturating_sub(record.root_slot) as f64;
        self.weighted_commission_sum += weight * f64::from(record.commission);

        if let (Some(credits), Some(expected_epoch), Some(ideal)) = (
            &record.latest_epoch_credits,
            sample_epoch,
            ideal_epoch_credits,
        ) {
            if credits.epoch == expected_epoch {
                self.latest_epoch_vote_credits += credits.earned;
                if ideal > 0 {
                    self.weighted_vote_efficiency_sum +=
                        weight * ((credits.earned as f64 / ideal as f64) * 100.0).clamp(0.0, 100.0);
                }
            }
        }

        self.blocks_produced += record.current_epoch_blocks_produced;
        self.blocks_missed += record
            .current_epoch_leader_slots
            .saturating_sub(record.current_epoch_blocks_produced);
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
    let epoch_info = client
        .get_epoch_info()
        .await
        .context("getEpochInfo failed")?;

    let current_slot = epoch_info.absolute_slot;
    let epoch_start_slot = current_slot.saturating_sub(epoch_info.slot_index);
    let next_epoch_start_slot = epoch_start_slot + epoch_info.slots_in_epoch;

    let vote_accounts = client
        .get_vote_accounts(config.delinquent_slot_distance)
        .await
        .context("getVoteAccounts failed")?;

    let block_production = client
        .get_block_production(epoch_start_slot, current_slot)
        .await
        .context("getBlockProduction failed")?;

    let current_schedule = client
        .get_leader_schedule(current_slot)
        .await
        .context("getLeaderSchedule for current epoch failed")?;

    let next_schedule = client
        .get_leader_schedule(next_epoch_start_slot)
        .await
        .context("getLeaderSchedule for next epoch failed")?;

    let matcher = LabelMatcher::from_config(config);
    let build_context = RecordBuildContext {
        matcher: &matcher,
        current_slot,
        epoch_start_slot,
        by_identity: &block_production.by_identity,
        current_schedule: current_schedule.as_ref(),
        next_schedule: next_schedule.as_ref(),
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
    current_schedule: Option<&'a HashMap<String, Vec<u64>>>,
    next_schedule: Option<&'a HashMap<String, Vec<u64>>>,
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

    let ideal_epoch_credits = sample_epoch.and_then(|expected_epoch| {
        records
            .iter()
            .filter_map(|record| record.latest_epoch_credits.as_ref())
            .filter(|credits| credits.epoch == expected_epoch)
            .map(|credits| credits.earned)
            .max()
    });

    let mut scopes: BTreeMap<String, ScopeAccumulator> = BTreeMap::new();

    for record in records {
        for scope in &record.scopes {
            scopes.entry(scope.clone()).or_default().observe(
                record,
                slot,
                ideal_epoch_credits,
                sample_epoch,
            );
        }
    }

    let mut watched_validators = records
        .iter()
        .filter(|record| {
            record
                .scopes
                .iter()
                .any(|scope| scope == LABEL_SCOPE_WATCHED)
        })
        .map(|record| {
            build_watched_validator_metrics(record, slot, ideal_epoch_credits, sample_epoch)
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
) -> WatchedValidatorMetrics {
    let latest_epoch_vote_credits = record
        .latest_epoch_credits
        .as_ref()
        .filter(|credits| Some(credits.epoch) == sample_epoch)
        .map(|credits| credits.earned)
        .map_or(0, |earned| earned);
    let vote_efficiency_pct = if latest_epoch_vote_credits > 0 {
        ideal_epoch_credits
            .filter(|ideal| *ideal > 0)
            .map(|ideal| {
                ((latest_epoch_vote_credits as f64 / ideal as f64) * 100.0).clamp(0.0, 100.0)
            })
            .map_or(0.0, |efficiency| efficiency)
    } else {
        0.0
    };

    WatchedValidatorMetrics {
        identity: record.identity.clone(),
        vote_account: record.vote_account.clone(),
        status: record.status,
        scopes: record.scopes.clone(),
        activated_stake_lamports: record.activated_stake_lamports,
        latest_epoch_vote_credits,
        vote_efficiency_pct,
        last_vote_slot_lag: current_slot.saturating_sub(record.last_vote),
        root_slot_lag: current_slot.saturating_sub(record.root_slot),
        commission_pct: record.commission,
        blocks_produced: record.current_epoch_blocks_produced,
        blocks_missed: record
            .current_epoch_leader_slots
            .saturating_sub(record.current_epoch_blocks_produced),
        block_production_rate: ratio(
            record.current_epoch_blocks_produced,
            record.current_epoch_leader_slots,
        ),
        future_leader_slots: record.future_leader_slots,
    }
}

fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        return 0.0;
    }
    (numerator as f64 / denominator as f64) * 100.0
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlockProduction {
    by_identity: HashMap<String, (u64, u64)>,
}

#[cfg(test)]
mod tests {
    use super::{
        aggregate_records, EpochCreditsSample, ScopeMetrics, ValidatorRecord, ValidatorStatus,
        WatchedValidatorMetrics,
    };
    use anyhow::{anyhow, Result};

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
        assert_eq!(
            watched,
            &ScopeMetrics {
                current_count: 1,
                delinquent_count: 0,
                current_stake_lamports: 10,
                delinquent_stake_lamports: 0,
                latest_epoch_vote_credits: 10,
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
}
