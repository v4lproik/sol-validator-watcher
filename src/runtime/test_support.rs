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
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{net::TcpListener, task::JoinHandle};

pub(super) async fn start_capability_probe_server(
    missing_method: Option<&'static str>,
) -> Result<(String, JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind capability probe listener")?;
    let address = listener
        .local_addr()
        .context("failed to read capability probe listener address")?;
    let app = Router::new()
        .route("/", post(capability_probe_handler))
        .with_state(missing_method);
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    Ok((format!("http://{address}"), task))
}

async fn capability_probe_handler(
    State(missing_method): State<Option<&'static str>>,
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
    if missing_method == Some(method) {
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
