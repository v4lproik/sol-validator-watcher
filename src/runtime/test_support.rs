use anyhow::{Context, Result};
use axum::{
    extract::{Json, State},
    routing::post,
    Router,
};
use serde_json::{json, Value};
use std::{
    env,
    path::PathBuf,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{net::TcpListener, task::JoinHandle};

const HEALTHY: u8 = 0;
const FAILED: u8 = 1;
const BEHIND: u8 = 2;

#[derive(Clone)]
struct CapabilityProbeState {
    missing_method: Option<&'static str>,
    health_state: Arc<AtomicU8>,
}

pub(super) struct CapabilityProbeControl {
    health_state: Arc<AtomicU8>,
}

impl CapabilityProbeControl {
    pub(super) fn set_health_failure(&self, failed: bool) {
        self.health_state
            .store(if failed { FAILED } else { HEALTHY }, Ordering::Relaxed);
    }

    pub(super) fn set_health_behind(&self, behind: bool) {
        self.health_state
            .store(if behind { BEHIND } else { HEALTHY }, Ordering::Relaxed);
    }
}

pub(super) async fn start_capability_probe_server(
    missing_method: Option<&'static str>,
) -> Result<(String, JoinHandle<()>)> {
    let (url, _control, task) = start_controlled_capability_probe_server(missing_method).await?;
    Ok((url, task))
}

pub(super) async fn start_controlled_capability_probe_server(
    missing_method: Option<&'static str>,
) -> Result<(String, CapabilityProbeControl, JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind capability probe listener")?;
    let address = listener
        .local_addr()
        .context("failed to read capability probe listener address")?;
    let health_state = Arc::new(AtomicU8::new(HEALTHY));
    let state = CapabilityProbeState {
        missing_method,
        health_state: Arc::clone(&health_state),
    };
    let app = Router::new()
        .route("/", post(capability_probe_handler))
        .with_state(state);
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let control = CapabilityProbeControl { health_state };

    Ok((format!("http://{address}"), control, task))
}

async fn capability_probe_handler(
    State(state): State<CapabilityProbeState>,
    Json(request): Json<Value>,
) -> Json<Value> {
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = request
        .get("params")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if method == "getVoteAccounts"
        && params
            .first()
            .and_then(|config| config.get("delinquentSlotDistance"))
            .and_then(Value::as_u64)
            == Some(256)
    {
        return Json(json!({
            "jsonrpc": "2.0",
            "error": {
                "code": -32602,
                "message": "unsupported delinquent slot distance"
            },
            "id": 1
        }));
    }
    if method == "getHealth" {
        let health_state = state.health_state.load(Ordering::Relaxed);
        if health_state == FAILED {
            return Json(json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32000,
                    "message": "temporary upstream failure"
                },
                "id": 1
            }));
        }
        if health_state == BEHIND {
            return Json(json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32005,
                    "message": "Node is behind by 42 slots",
                    "data": { "numSlotsBehind": 42 }
                },
                "id": 1
            }));
        }
    }
    if state.missing_method == Some(method) {
        return Json(json!({
            "jsonrpc": "2.0",
            "error": {
                "code": -32601,
                "message": "method not found"
            },
            "id": 1
        }));
    }

    let result = match method {
        "getHealth" => json!("ok"),
        "getIdentity" => json!({ "identity": "id-1" }),
        "getVersion" => json!({
            "solana-core": "3.1.8",
            "feature-set": 2_891_131_721u32
        }),
        "getBlockHeight" => json!(1_233),
        "getEpochInfo" => json!({
            "epoch": 1,
            "slotIndex": 1,
            "slotsInEpoch": 32,
            "absoluteSlot": 33
        }),
        "getVoteAccounts" => json!({
            "current": [{
                "votePubkey": "vote-1",
                "nodePubkey": "id-1",
                "activatedStake": 1_500_000_000u64,
                "commission": 5,
                "epochCredits": [[0, 20, 10]],
                "lastVote": 32,
                "rootSlot": 31
            }],
            "delinquent": []
        }),
        "getSlotLeader" => json!("probe-validator-identity"),
        "getLeaderSchedule" => Value::Null,
        "getRecentPerformanceSamples" => json!([]),
        "getBlockProduction" => json!({
            "context": { "slot": 33 },
            "value": {
                "byIdentity": {},
                "range": { "firstSlot": 32, "lastSlot": 33 }
            }
        }),
        "getMultipleAccounts" => json!({
            "context": { "slot": 33 },
            "value": [
                { "lamports": 3_000_000_000u64, "space": 0 },
                { "lamports": 2_500_000_000u64, "space": 3762 }
            ]
        }),
        "getMinimumBalanceForRentExemption" | "minimumLedgerSlot" => json!(0),
        "getInflationReward" => json!([null]),
        _ => {
            return Json(json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32601,
                    "message": "method not found"
                },
                "id": 1
            }));
        }
    };
    Json(json!({
        "jsonrpc": "2.0",
        "result": result,
        "id": 1
    }))
}

pub(super) fn unique_test_dir(prefix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    env::temp_dir().join(format!(
        "sol-validator-watcher-{}-{}-{}",
        prefix,
        std::process::id(),
        nonce
    ))
}
