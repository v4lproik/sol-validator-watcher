use super::{modes_for, start_mock_rpc_server};
use crate::{
    collector::{collect_snapshot, ensure_economics_supported, TrilliumClient},
    config::{Config, ConnectorConfig},
    mode::{CommandId, ProviderFeature, SolanaRpcCompatibility},
    source::SourceChain,
    watcher_state::RuntimeTelemetry,
};
use anyhow::{Context, Result};
use axum::{
    extract::{Path, State},
    http::{
        header::{AUTHORIZATION, IF_MODIFIED_SINCE},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::{oneshot, Mutex},
    task::JoinHandle,
    time::{sleep, timeout},
};

const LAST_MODIFIED: &str = "Sun, 08 Nov 2026 06:48:10 GMT";
const CAPTURE_RESPONSE: &str = r#"[
    {
        "epoch": 8,
        "vote_account_pubkey": "vote-1",
        "mev_earned": 1.0,
        "mev_to_validator": 0.1,
        "mev_to_stakers": 0.8,
        "mev_to_jito_block_engine": 0.05,
        "mev_to_jito_tip_router": 0.05,
        "avg_mev_per_leader_slot": 0.01,
        "mev_commission": 1000,
        "total_jito_tips": 1.05,
        "total_jito_tip_count": 100,
        "leader_slots": 100,
        "blocks_produced": 98
    },
    {
        "epoch": 9,
        "vote_account_pubkey": "vote-1",
        "mev_earned": 1.25,
        "mev_to_validator": 0.25,
        "mev_to_stakers": 0.9,
        "mev_to_jito_block_engine": 0.05,
        "mev_to_jito_tip_router": 0.05,
        "avg_mev_per_leader_slot": 0.0125,
        "mev_commission": 2000,
        "total_jito_tips": 1.3,
        "total_jito_tip_count": 125,
        "leader_slots": 100,
        "blocks_produced": 99,
        "total_block_rewards_after_burn": 2.1,
        "validator_priority_fees": 1.2,
        "validator_signature_fees": 0.2,
        "delegator_priority_fees": 0.3,
        "total_priority_fees": 1.5,
        "avg_mev_per_block": 0.012,
        "median_mev_per_block": 0.009,
        "avg_priority_fees_per_block": 0.015,
        "median_priority_fees_per_block": 0.011,
        "avg_signature_fees_per_block": 0.002,
        "median_signature_fees_per_block": 0.001,
        "avg_jito_tips_per_block": 0.013,
        "median_jito_tips_per_block": 0.008,
        "priority_fee_commission": 1500,
        "activated_stake": 200000,
        "avg_tx_per_block": 100,
        "avg_user_tx_per_block": 80,
        "avg_vote_tx_per_block": 20,
        "median_tx_per_block": 90,
        "median_user_tx_per_block": 70,
        "avg_tx_success_rate": 80,
        "avg_user_tx_success_rate": 75,
        "avg_cu_per_block": 14000000,
        "median_cu_per_block": 13000000,
        "total_successful_tx": 8000,
        "total_failed_tx": 2000,
        "total_successful_user_tx": 6000,
        "total_failed_user_tx": 2000
    }
]"#;

#[derive(Clone)]
struct MockTrilliumState {
    status: StatusCode,
    body: Arc<str>,
    conditional: bool,
    requests: Arc<AtomicUsize>,
    conditional_headers: Arc<Mutex<Vec<Option<String>>>>,
    authorization_headers: Arc<Mutex<Vec<Option<String>>>>,
}

#[test]
fn trillium_client_constructor_rejects_bad_urls_without_exposing_queries() -> Result<()> {
    let secret = "trillium-constructor-secret";
    for url in [
        format!("ftp://trillium.example/?api-key={secret}"),
        format!("http:///api?api-key={secret}"),
    ] {
        let Err(error) = TrilliumClient::try_new(&crate::config::HttpTransportConfig {
            url,
            request_timeout_sec: 1,
            connect_timeout_sec: 1,
            ..crate::config::HttpTransportConfig::default()
        }) else {
            anyhow::bail!("invalid Trillium API URL should fail during client construction");
        };
        let message = format!("{error:#}");
        assert!(message.contains("Trillium API URL must use http:// or https:// with a host"));
        assert!(!message.contains(secret));
    }
    Ok(())
}

#[tokio::test]
async fn trillium_transport_applies_custom_headers_without_exposing_credentials() -> Result<()> {
    let (api_url, server, _, _, authorization_headers) =
        start_mock_trillium_server(StatusCode::OK, CAPTURE_RESPONSE, false).await?;
    let secret = "trillium-header-secret";
    let transport: crate::config::HttpTransportConfig = toml::from_str(&format!(
        r#"
url = "{api_url}"
request_timeout_sec = 5
connect_timeout_sec = 5
headers = {{ authorization = "Bearer {secret}" }}
"#
    ))?;
    let debug = format!("{transport:?}");
    assert!(debug.contains("authorization"));
    assert!(!debug.contains(secret));
    assert!(!debug.contains(&api_url));
    let client = TrilliumClient::try_new(&transport)?;

    assert!(client.get_latest_mev_capture("vote-1").await?.is_some());
    server.abort();
    assert_eq!(
        authorization_headers.lock().await.as_slice(),
        &[Some(format!("Bearer {secret}"))]
    );
    Ok(())
}

#[tokio::test]
async fn trillium_transport_rejects_oversized_responses() -> Result<()> {
    let oversized = format!("[{}]", " ".repeat(1_048_576));
    let (api_url, server, _, _, _) =
        start_mock_trillium_server(StatusCode::OK, oversized, false).await?;
    let client = TrilliumClient::try_new(&crate::config::HttpTransportConfig::new(api_url))?;

    let Err(error) = client.get_latest_mev_capture("vote-1").await else {
        anyhow::bail!("oversized Trillium response should fail");
    };
    server.abort();

    assert!(error
        .to_string()
        .contains("Trillium validator rewards response exceeds 1048576 bytes"));
    Ok(())
}

#[tokio::test]
async fn trillium_transport_enforces_total_request_timeout() -> Result<()> {
    let (api_url, server, request_received) = start_stalled_trillium_server().await?;
    let outcome = async {
        let client = TrilliumClient::try_new(&crate::config::HttpTransportConfig {
            url: api_url,
            request_timeout_sec: 1,
            connect_timeout_sec: 1,
            ..crate::config::HttpTransportConfig::default()
        })?;
        let (accepted, result) = tokio::join!(
            timeout(Duration::from_secs(1), request_received),
            timeout(
                Duration::from_secs(2),
                client.get_latest_mev_capture("vote-1")
            )
        );
        Ok::<_, anyhow::Error>((accepted, result))
    }
    .await;
    server.abort();

    let (accepted, result) = outcome?;
    accepted
        .context("mock Trillium API did not accept the request before its deadline")?
        .context("mock Trillium API dropped the request acceptance signal")?;
    let error = match result {
        Ok(Err(error)) => error,
        Ok(Ok(_)) => anyhow::bail!("stalled Trillium response must not complete successfully"),
        Err(_) => anyhow::bail!("Trillium request did not return before the outer deadline"),
    };
    assert!(error
        .to_string()
        .contains("Trillium validator rewards request failed"));
    Ok(())
}
#[tokio::test]
async fn trillium_client_decodes_latest_capture_and_conditionally_revalidates() -> Result<()> {
    let (api_url, server, requests, conditional_headers, _) =
        start_mock_trillium_server(StatusCode::OK, CAPTURE_RESPONSE, true).await?;
    let client = TrilliumClient::try_new_for_test(
        &crate::config::HttpTransportConfig::new(api_url),
        Duration::ZERO,
    )?;

    let capture = client
        .get_latest_mev_capture("vote-1")
        .await?
        .context("expected latest Trillium capture")?;
    let revalidated = client
        .get_latest_mev_capture("vote-1")
        .await?
        .context("expected revalidated Trillium capture")?;
    server.abort();

    assert_eq!(capture, revalidated);
    assert_eq!(capture.epoch, 9);
    assert_eq!(capture.captured_sol, Some(1.25));
    assert_eq!(capture.per_leader_slot_sol, Some(0.0125));
    assert_eq!(capture.commission_pct, Some(20.0));
    assert_eq!(capture.total_jito_tip_count, Some(125));
    assert_eq!(capture.activated_stake_sol, Some(200_000.0));
    assert_eq!(capture.block_rewards.after_burn_sol, Some(2.1));
    assert_eq!(
        capture.block_rewards.priority_fee_commission_pct,
        Some(15.0)
    );
    assert_eq!(capture.transactions.successful, Some(8_000));
    assert_eq!(
        capture.transactions.average_compute_units_per_block,
        Some(14_000_000.0)
    );
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    assert_eq!(
        conditional_headers.lock().await.as_slice(),
        &[None, Some(LAST_MODIFIED.to_string())]
    );
    Ok(())
}

#[tokio::test]
async fn trillium_client_caches_validator_not_found_as_valid_absence() -> Result<()> {
    let (api_url, server, requests, _, _) = start_mock_trillium_server(
        StatusCode::NOT_FOUND,
        r#"{"error":"Validator not found in the last 10 epochs"}"#,
        false,
    )
    .await?;
    let client = TrilliumClient::try_new(&crate::config::HttpTransportConfig::new(api_url))?;

    assert_eq!(client.get_latest_mev_capture("vote-1").await?, None);
    assert_eq!(client.get_latest_mev_capture("vote-1").await?, None);
    server.abort();
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn trillium_client_does_not_cache_unrecognized_not_found_responses() -> Result<()> {
    let (api_url, server, requests, _, _) =
        start_mock_trillium_server(StatusCode::NOT_FOUND, r#"{"error":"unknown path"}"#, false)
            .await?;
    let client = TrilliumClient::try_new(&crate::config::HttpTransportConfig::new(api_url))?;

    for _ in 0..2 {
        let error = client
            .get_latest_mev_capture("vote-1")
            .await
            .err()
            .context("unrecognized HTTP 404 should fail")?;
        assert!(error
            .to_string()
            .contains("Trillium validator rewards request returned HTTP 404"));
    }
    server.abort();
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn economics_snapshot_publishes_trillium_capture_with_source_provenance() -> Result<()> {
    let (rpc_url, rpc_server) = start_mock_rpc_server().await?;
    let (api_url, trillium_server, requests, _, _) =
        start_mock_trillium_server(StatusCode::OK, CAPTURE_RESPONSE, false).await?;
    let config = snapshot_config(rpc_url, api_url);
    let sources = SourceChain::from_config(&config, RuntimeTelemetry::unobserved())?;
    ensure_economics_supported(&sources, &config).await?;
    assert_eq!(requests.load(Ordering::SeqCst), 0);

    let snapshot = collect_snapshot(&sources, &config).await?;
    let capture = snapshot
        .economics
        .context("economics mode should publish metrics")?
        .trillium_mev_capture
        .context("configured Trillium route should publish capture")?;
    assert_eq!(capture.epoch, 9);
    assert_eq!(capture.validator_sol, Some(0.25));
    assert_eq!(capture.block_rewards.priority_fees_sol, Some(1.5));
    assert_eq!(capture.transactions.successful, Some(8_000));
    assert!(snapshot.data_sources.iter().any(|source| {
        source.command == CommandId::TrilliumMevCapture
            && source.connector.as_ref() == "trillium-api"
            && source.precedence == 1
    }));
    assert_eq!(requests.load(Ordering::SeqCst), 1);

    rpc_server.abort();
    trillium_server.abort();
    Ok(())
}

#[tokio::test]
async fn trillium_failure_omits_capture_without_failing_economics_snapshot() -> Result<()> {
    let (rpc_url, rpc_server) = start_mock_rpc_server().await?;
    let (api_url, trillium_server, requests, _, _) =
        start_mock_trillium_server(StatusCode::TOO_MANY_REQUESTS, "rate limited", false).await?;
    let config = snapshot_config(rpc_url, api_url);
    let sources = SourceChain::from_config(&config, RuntimeTelemetry::unobserved())?;
    ensure_economics_supported(&sources, &config).await?;

    let snapshot = collect_snapshot(&sources, &config).await?;
    assert!(snapshot.economics.is_some());
    assert_eq!(
        snapshot
            .economics
            .as_ref()
            .and_then(|economics| economics.trillium_mev_capture),
        None
    );
    assert!(!snapshot
        .data_sources
        .iter()
        .any(|source| source.command == CommandId::TrilliumMevCapture));
    assert_eq!(requests.load(Ordering::SeqCst), 1);

    rpc_server.abort();
    trillium_server.abort();
    Ok(())
}

fn snapshot_config(rpc_url: String, provider_api_url: String) -> Config {
    let mut modes = modes_for(&["rpc"], true);
    modes.economics.commands.trillium_mev_capture = vec!["trillium-api".to_string()];
    Config {
        network: "trillium-test".to_string(),
        connectors: vec![
            ConnectorConfig {
                name: "rpc".to_string(),
                provider: "mock-solana".to_string(),
                features: vec![ProviderFeature::HistoricalData],
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
                rpc: Some(crate::config::RpcTransportConfig::new(rpc_url)),
                ..ConnectorConfig::default()
            },
            ConnectorConfig {
                name: "trillium-api".to_string(),
                provider: "trillium".to_string(),
                features: vec![ProviderFeature::TrilliumMevCapture],
                provider_api: Some(crate::config::HttpTransportConfig::new(provider_api_url)),
                ..ConnectorConfig::default()
            },
        ],
        modes,
        ..Config::default()
    }
}

async fn start_stalled_trillium_server() -> Result<(String, JoinHandle<()>, oneshot::Receiver<()>)>
{
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind stalled Trillium API listener")?;
    let address = listener
        .local_addr()
        .context("failed to read stalled Trillium API address")?;
    let (request_received, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        if listener.accept().await.is_ok() {
            let _ = request_received.send(());
            sleep(Duration::from_secs(5)).await;
        }
    });
    Ok((format!("http://{address}"), task, receiver))
}

type MockTrilliumServer = (
    String,
    JoinHandle<()>,
    Arc<AtomicUsize>,
    Arc<Mutex<Vec<Option<String>>>>,
    Arc<Mutex<Vec<Option<String>>>>,
);

async fn start_mock_trillium_server(
    status: StatusCode,
    body: impl Into<Arc<str>>,
    conditional: bool,
) -> Result<MockTrilliumServer> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind mock Trillium API listener")?;
    let address = listener
        .local_addr()
        .context("failed to read mock Trillium API listener address")?;
    let requests = Arc::new(AtomicUsize::new(0));
    let conditional_headers = Arc::new(Mutex::new(Vec::new()));
    let authorization_headers = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route(
            "/validator_rewards/{vote_account}",
            get(mock_trillium_handler),
        )
        .with_state(MockTrilliumState {
            status,
            body: body.into(),
            conditional,
            requests: Arc::clone(&requests),
            conditional_headers: Arc::clone(&conditional_headers),
            authorization_headers: Arc::clone(&authorization_headers),
        });
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok((
        format!("http://{address}"),
        task,
        requests,
        conditional_headers,
        authorization_headers,
    ))
}

async fn mock_trillium_handler(
    State(state): State<MockTrilliumState>,
    Path(vote_account): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    state.requests.fetch_add(1, Ordering::SeqCst);
    let if_modified_since = headers
        .get(IF_MODIFIED_SINCE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    state
        .conditional_headers
        .lock()
        .await
        .push(if_modified_since.clone());
    let authorization = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    state.authorization_headers.lock().await.push(authorization);
    if vote_account != "vote-1" {
        return StatusCode::NOT_FOUND.into_response();
    }
    if state.conditional && if_modified_since.as_deref() == Some(LAST_MODIFIED) {
        return StatusCode::NOT_MODIFIED.into_response();
    }

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        axum::http::header::LAST_MODIFIED,
        HeaderValue::from_static(LAST_MODIFIED),
    );
    (state.status, response_headers, state.body.to_string()).into_response()
}
