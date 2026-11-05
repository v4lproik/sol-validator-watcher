use super::{
    aggregate_records, build_economics_metrics, build_record, build_watched_validator_metrics,
    collect_snapshot, ensure_economics_supported, estimated_epoch_end_unixtime_at, ratio,
    resolve_economics_target, reward_epoch_block_range, AccountBalances, BlockProduction,
    ClusterPerformanceMetrics, EconomicsTarget, EpochCreditsSample, GetBlockProductionConfig,
    RecentBlockFeeRewards, RecordBuildContext, RewardSample, RpcClient, RpcValueResponse,
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
    metrics::Metrics,
    mode::{CommandId, ModeId, ProviderFeature, SolanaRpcCompatibility},
    source::SourceChain,
    tasks::{TaskExit, TaskManager},
    watcher_state::WatcherStateMailbox,
};
use anyhow::{anyhow, Context, Result};
use axum::{
    extract::{Json, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Router,
};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};
use tokio::{net::TcpListener, sync::Mutex, task::JoinHandle};

#[derive(Clone, Copy)]
enum MockRpcProfile {
    Default,
    StrictCapabilityParams,
    NoAccountSpace,
    NoCurrentBlockProduction,
    Behind,
    BehindHttp503,
    Unhealthy,
    Delayed,
    NoEpochInfo,
    NoInflationRewards,
    NoLeaderSchedule,
    NoMinimumLedgerSlot,
    NoBlockFeeRewards,
    WrongBlockFeeRewardIdentity,
    NoPerformanceSamples,
    InvalidPerformanceTransactionSplit,
    MissingPerformanceTransactionSplit,
    ZeroPerformanceSamplePeriod,
    NoSlotLeader,
    NullPerformanceSamples,
    MissingPerformanceSamplesResult,
}

#[derive(Clone)]
struct MockRpcState {
    profile: MockRpcProfile,
    methods: Arc<Mutex<Vec<String>>>,
    authorization_headers: Arc<Mutex<Vec<Option<String>>>>,
    requests: Arc<Mutex<Vec<Value>>>,
}

mod aggregate;
mod collection;
mod jito;
mod rpc;

fn rpc_transport(url: &str, timeout_sec: u64) -> crate::config::RpcTransportConfig {
    crate::config::RpcTransportConfig {
        url: url.to_string(),
        request_timeout_sec: timeout_sec,
        connect_timeout_sec: timeout_sec,
        ..crate::config::RpcTransportConfig::default()
    }
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
    let (url, task, methods, _) = start_header_recording_mock_rpc_server(profile).await?;
    Ok((url, task, methods))
}

type HeaderRecordingMockRpcServer = (
    String,
    JoinHandle<()>,
    Arc<Mutex<Vec<String>>>,
    Arc<Mutex<Vec<Option<String>>>>,
);

type RequestRecordingMockRpcServer = (String, JoinHandle<()>, Arc<Mutex<Vec<Value>>>);

type MockRpcServerRecordings = (
    String,
    JoinHandle<()>,
    Arc<Mutex<Vec<String>>>,
    Arc<Mutex<Vec<Option<String>>>>,
    Arc<Mutex<Vec<Value>>>,
);

async fn start_request_recording_mock_rpc_server(
    profile: MockRpcProfile,
) -> Result<RequestRecordingMockRpcServer> {
    let (url, task, _, _, requests) = start_mock_rpc_server_with_recordings(profile).await?;
    Ok((url, task, requests))
}

async fn start_header_recording_mock_rpc_server(
    profile: MockRpcProfile,
) -> Result<HeaderRecordingMockRpcServer> {
    let (url, task, methods, authorization_headers, _) =
        start_mock_rpc_server_with_recordings(profile).await?;
    Ok((url, task, methods, authorization_headers))
}

async fn start_mock_rpc_server_with_recordings(
    profile: MockRpcProfile,
) -> Result<MockRpcServerRecordings> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind mock RPC listener")?;
    let address = listener
        .local_addr()
        .context("failed to read mock RPC listener address")?;
    let methods = Arc::new(Mutex::new(Vec::new()));
    let authorization_headers = Arc::new(Mutex::new(Vec::new()));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/", post(mock_rpc_handler))
        .with_state(MockRpcState {
            profile,
            methods: Arc::clone(&methods),
            authorization_headers: Arc::clone(&authorization_headers),
            requests: Arc::clone(&requests),
        });
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    Ok((
        format!("http://{address}"),
        task,
        methods,
        authorization_headers,
        requests,
    ))
}
fn capability_probe_params_are_valid(method: &str, params: &[Value]) -> bool {
    let actual = Value::Array(params.to_vec());
    match method {
        "getHealth" | "getIdentity" | "getVersion" | "minimumLedgerSlot" => actual == json!([]),
        "getBlockHeight" => actual == json!([{ "commitment": "confirmed" }]),
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
        "getBlock" => {
            params.first().and_then(Value::as_u64).is_some()
                && params.get(1)
                    == Some(&json!({
                        "commitment": "confirmed",
                        "transactionDetails": "none",
                        "rewards": true
                    }))
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
        "getMinimumBalanceForRentExemption" => actual == json!([0, { "commitment": "confirmed" }]),
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
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> impl IntoResponse {
    let profile = state.profile;
    let authorization = headers
        .get(reqwest::header::AUTHORIZATION.as_str())
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    if let Some(batch) = request.as_array() {
        state.requests.lock().await.push(request.clone());
        state.authorization_headers.lock().await.push(authorization);
        state.methods.lock().await.extend(batch.iter().map(|item| {
            item.get("method")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        }));
        if matches!(profile, MockRpcProfile::Delayed) {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
        let responses = batch
            .iter()
            .map(|item| mock_batch_response(profile, item))
            .collect();
        return (StatusCode::OK, Json(Value::Array(responses))).into_response();
    }

    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    state.methods.lock().await.push(method.to_string());
    state.requests.lock().await.push(request.clone());
    state.authorization_headers.lock().await.push(authorization);
    if matches!(profile, MockRpcProfile::Delayed) {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
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
            return (StatusCode::OK, Json(json!({ "jsonrpc": "2.0", "id": 1 }))).into_response();
        }
    }

    match method {
        "getHealth" => {
            if matches!(
                profile,
                MockRpcProfile::Behind | MockRpcProfile::BehindHttp503
            ) {
                let status = if matches!(profile, MockRpcProfile::BehindHttp503) {
                    StatusCode::SERVICE_UNAVAILABLE
                } else {
                    StatusCode::OK
                };
                return (
                    status,
                    Json(json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32005,
                            "message": "Node is behind by 42 slots",
                            "data": { "numSlotsBehind": 42 }
                        },
                        "id": 1
                    })),
                )
                    .into_response();
            }
            let health = if matches!(profile, MockRpcProfile::Unhealthy) {
                "unhealthy"
            } else {
                "ok"
            };
            (StatusCode::OK, Json(rpc_success(json!(health)))).into_response()
        }
        "getIdentity" => (
            StatusCode::OK,
            Json(rpc_success(json!({ "identity": "id-1" }))),
        )
            .into_response(),
        "getVersion" => (
            StatusCode::OK,
            Json(rpc_success(json!({
                "solana-core": "3.1.8",
                "feature-set": 2_891_131_721u32
            }))),
        )
            .into_response(),
        "getBlockHeight" => (StatusCode::OK, Json(rpc_success(json!(1_233)))).into_response(),
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
            let mut sample = json!({
                "slot": 810,
                "numTransactions": 1000,
                "numSlots": 100,
                "samplePeriodSecs": if matches!(
                    profile,
                    MockRpcProfile::ZeroPerformanceSamplePeriod
                ) {
                    0
                } else {
                    40
                }
            });
            if !matches!(profile, MockRpcProfile::MissingPerformanceTransactionSplit) {
                sample["numNonVoteTransactions"] = json!(if matches!(
                    profile,
                    MockRpcProfile::InvalidPerformanceTransactionSplit
                ) {
                    1_001
                } else {
                    100
                });
            }
            (StatusCode::OK, Json(rpc_success(json!([sample])))).into_response()
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

            if first_slot.is_none() && matches!(profile, MockRpcProfile::NoCurrentBlockProduction) {
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

fn mock_batch_response(profile: MockRpcProfile, request: &Value) -> Value {
    let id = request
        .get("id")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = request
        .get("params")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if method != "getBlock" {
        return rpc_error_with_id(id, -32601, "method not found");
    }
    if matches!(profile, MockRpcProfile::StrictCapabilityParams)
        && !capability_probe_params_are_valid(method, &params)
    {
        return rpc_error_with_id(id, -32602, "invalid capability probe params");
    }
    if matches!(profile, MockRpcProfile::NoBlockFeeRewards) {
        return rpc_error_with_id(id, -32601, "method not found");
    }

    let slot = params.first().and_then(Value::as_u64).unwrap_or_default();
    if slot == 802 {
        return rpc_success_with_id(id, Value::Null);
    }
    let identity = if matches!(profile, MockRpcProfile::WrongBlockFeeRewardIdentity) {
        "other-id"
    } else {
        "id-1"
    };
    let lamports = match slot {
        800 => 1_000_000,
        805 => 3_000_000,
        _ => 2_000_000,
    };
    rpc_success_with_id(
        id,
        json!({
            "blockHeight": slot,
            "blockTime": 1_700_000_000,
            "blockhash": "blockhash",
            "parentSlot": slot.saturating_sub(1),
            "previousBlockhash": "previous",
            "rewards": [{
                "pubkey": identity,
                "lamports": lamports,
                "postBalance": 1_000_000_000u64,
                "rewardType": "Fee",
                "commission": null
            }]
        }),
    )
}

fn rpc_success(result: Value) -> Value {
    rpc_success_with_id(1, result)
}

fn rpc_success_with_id(id: u64, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "result": result,
        "id": id
    })
}

fn rpc_error(code: i64, message: &str) -> Value {
    rpc_error_with_id(1, code, message)
}

fn rpc_error_with_id(id: u64, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "error": {
            "code": code,
            "message": message
        },
        "id": id
    })
}

fn modes_for(connectors: &[&str], economics_enabled: bool) -> ModesConfig {
    let route = || connectors.iter().map(|name| (*name).to_string()).collect();
    ModesConfig {
        health: HealthModeConfig {
            commands: HealthCommands {
                rpc_health: route(),
                ws_health: vec![],
                node_status: route(),
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
                reward_epoch_block_production: economics_enabled.then(route).unwrap_or_default(),
                recent_block_fee_rewards: vec![],
                jito_mev_rewards: vec![],
            },
        },
    }
}
