use super::start_mock_rpc_server;
use crate::{
    collector::{
        collect_snapshot, command::GetJitoMevReward, ensure_economics_supported, JitoClient,
    },
    config::{Config, ConnectorConfig, EconomicsCommands, EconomicsModeConfig, ModesConfig},
    mode::{CommandId, ProviderFeature, SolanaRpcCompatibility},
    source::SourceChain,
    RuntimeTelemetry,
};
use anyhow::{Context, Result};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
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

#[derive(Clone)]
struct MockJitoState {
    status: StatusCode,
    body: &'static str,
    requests: Arc<AtomicUsize>,
    authorization_headers: Arc<Mutex<Vec<Option<String>>>>,
}

#[derive(Clone)]
struct DelayedMockJitoState {
    request_received: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

#[test]
fn jito_client_constructor_rejects_bad_urls_without_exposing_queries() -> Result<()> {
    let secret = "jito-constructor-secret";
    for (url, expected) in [
        (
            format!("ftp://jito.example/?api-key={secret}"),
            "Jito API URL must use http:// or https:// with a host",
        ),
        (
            format!("http:///api?api-key={secret}"),
            "Jito API URL must use http:// or https:// with a host",
        ),
    ] {
        let Err(error) = JitoClient::try_new(&crate::config::HttpTransportConfig {
            url,
            request_timeout_sec: 1,
            connect_timeout_sec: 1,
            ..crate::config::HttpTransportConfig::default()
        }) else {
            anyhow::bail!("invalid Jito API URL should fail during client construction");
        };
        let message = format!("{error:#}");
        assert!(message.contains(expected), "{message}");
        assert!(
            !message.contains(secret),
            "Jito constructor errors must redact secret URL query parameters"
        );
    }
    Ok(())
}

#[tokio::test]
async fn jito_client_selects_and_caches_one_reward_epoch() -> Result<()> {
    let (api_url, server, requests) = start_mock_jito_server(
        StatusCode::OK,
        r#"[
            {"epoch": 9, "mev_commission_bps": 500, "mev_rewards": 2000000000},
            {"epoch": 8, "mev_commission_bps": 800, "mev_rewards": 123456789}
        ]"#,
    )
    .await?;
    let client = JitoClient::try_new(&crate::config::HttpTransportConfig {
        url: api_url,
        request_timeout_sec: 5,
        connect_timeout_sec: 5,
        ..crate::config::HttpTransportConfig::default()
    })?;

    let reward = client
        .get_mev_reward("vote-1", 8)
        .await?
        .context("expected epoch 8 reward")?;
    assert_eq!(reward.epoch, 8);
    assert_eq!(reward.reward_lamports, 123_456_789);
    assert_eq!(reward.commission_bps, 800);
    assert_eq!(client.get_mev_reward("vote-1", 8).await?, Some(reward));
    assert_eq!(requests.load(Ordering::SeqCst), 1);

    assert_eq!(client.get_mev_reward("vote-1", 7).await?, None);
    assert_eq!(client.get_mev_reward("vote-1", 7).await?, None);
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    server.abort();
    Ok(())
}

#[tokio::test]
async fn jito_transport_applies_custom_headers_without_exposing_credentials() -> Result<()> {
    let (api_url, server, _, authorization_headers) = start_header_recording_mock_jito_server(
        StatusCode::OK,
        r#"[{"epoch": 8, "mev_commission_bps": 800, "mev_rewards": 123456789}]"#,
    )
    .await?;
    let secret = "jito-header-secret";
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
    let client = JitoClient::try_new(&transport)?;

    assert!(client.get_mev_reward("vote-1", 8).await?.is_some());
    server.abort();
    assert_eq!(
        authorization_headers.lock().await.as_slice(),
        &[Some(format!("Bearer {secret}"))]
    );
    Ok(())
}

#[tokio::test]
async fn jito_transport_enforces_total_request_timeout() -> Result<()> {
    let (api_url, server, request_received) = start_delayed_mock_jito_server().await?;
    let outcome = async {
        let client = JitoClient::try_new(&crate::config::HttpTransportConfig {
            url: api_url,
            request_timeout_sec: 1,
            connect_timeout_sec: 1,
            ..crate::config::HttpTransportConfig::default()
        })?;
        let (accepted, result) = tokio::join!(
            timeout(Duration::from_secs(1), request_received),
            timeout(Duration::from_secs(2), client.get_mev_reward("vote-1", 8))
        );
        Ok::<_, anyhow::Error>((accepted, result))
    }
    .await;
    server.abort();

    let (accepted, result) = outcome?;
    accepted
        .context("mock Jito API did not accept the request before its deadline")?
        .context("mock Jito API dropped the request acceptance signal")?;
    let error = match result {
        Ok(Err(error)) => error,
        Ok(Ok(_)) => anyhow::bail!("delayed Jito response must not complete successfully"),
        Err(_) => anyhow::bail!("Jito request did not return before the outer deadline"),
    };
    assert!(
        error.chain().any(|cause| cause
            .downcast_ref::<reqwest::Error>()
            .is_some_and(reqwest::Error::is_timeout)),
        "Jito request must report a transport timeout: {error:#}"
    );
    Ok(())
}

#[tokio::test]
async fn jito_reward_command_falls_back_to_the_next_connector() -> Result<()> {
    let (failed_url, failed_server, failed_requests) =
        start_mock_jito_server(StatusCode::SERVICE_UNAVAILABLE, "unavailable").await?;
    let (success_url, success_server, success_requests) = start_mock_jito_server(
        StatusCode::OK,
        r#"[{"epoch": 8, "mev_commission_bps": 1000, "mev_rewards": 250000000}]"#,
    )
    .await?;
    let config = Config {
        connectors: vec![
            jito_connector("primary-jito", "primary-provider", failed_url),
            jito_connector("fallback-jito", "fallback-provider", success_url),
        ],
        modes: ModesConfig {
            economics: EconomicsModeConfig {
                enabled: true,
                commands: EconomicsCommands {
                    jito_mev_rewards: vec!["primary-jito".to_string(), "fallback-jito".to_string()],
                    ..EconomicsCommands::default()
                },
                ..EconomicsModeConfig::default()
            },
            ..ModesConfig::default()
        },
        ..Config::default()
    };
    let sources = SourceChain::from_config(&config, RuntimeTelemetry::unobserved())?;

    let resolved = sources
        .required(&GetJitoMevReward {
            vote_account: "vote-1",
            epoch: 8,
        })
        .await?;
    assert_eq!(resolved.source.connector.as_ref(), "fallback-jito");
    assert_eq!(resolved.source.precedence, 2);
    assert_eq!(
        resolved
            .value
            .context("expected fallback reward")?
            .reward_lamports,
        250_000_000
    );
    assert_eq!(failed_requests.load(Ordering::SeqCst), 1);
    assert_eq!(success_requests.load(Ordering::SeqCst), 1);

    failed_server.abort();
    success_server.abort();
    Ok(())
}

#[tokio::test]
async fn jito_transport_errors_do_not_expose_secret_query_parameters() -> Result<()> {
    let secret = "provider-secret";
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind refused Jito API address")?;
    let address = listener
        .local_addr()
        .context("failed to read refused Jito API address")?;
    drop(listener);
    let config = Config {
        connectors: vec![jito_connector(
            "jito-api",
            "jito",
            format!("http://{address}/api/v1?api-key={secret}"),
        )],
        modes: ModesConfig {
            economics: EconomicsModeConfig {
                enabled: true,
                commands: EconomicsCommands {
                    jito_mev_rewards: vec!["jito-api".to_string()],
                    ..EconomicsCommands::default()
                },
                ..EconomicsModeConfig::default()
            },
            ..ModesConfig::default()
        },
        ..Config::default()
    };
    let sources = SourceChain::from_config(&config, RuntimeTelemetry::unobserved())?;

    let error = match sources
        .required(&GetJitoMevReward {
            vote_account: "vote-1",
            epoch: 8,
        })
        .await
    {
        Ok(_) => anyhow::bail!("connection refusal must fail the configured Jito route"),
        Err(error) => error,
    };
    let message = format!("{error:#}");
    assert!(message.contains("Jito validator rewards request failed"));
    assert!(message.contains("error sending request"));
    assert!(!message.contains(secret));
    Ok(())
}

#[tokio::test]
async fn economics_snapshot_collects_jito_reward_only_when_mode_is_enabled() -> Result<()> {
    let (rpc_url, rpc_server) = start_mock_rpc_server().await?;
    let (api_url, jito_server, requests) = start_mock_jito_server(
        StatusCode::OK,
        r#"[{"epoch": 8, "mev_commission_bps": 750, "mev_rewards": 375000000}]"#,
    )
    .await?;
    let config = snapshot_config(rpc_url, api_url, true);
    let sources = SourceChain::from_config(&config, RuntimeTelemetry::unobserved())?;
    ensure_economics_supported(&sources, &config).await?;

    let snapshot = collect_snapshot(&sources, &config).await?;
    let reward = snapshot
        .economics
        .context("economics mode should publish metrics")?
        .jito_mev_reward
        .context("configured Jito route should publish a settled reward")?;
    assert_eq!(reward.epoch, 8);
    assert_eq!(reward.reward_lamports, 375_000_000);
    assert_eq!(reward.commission_bps, 750);
    assert!(snapshot.data_sources.iter().any(|source| {
        source.command == CommandId::JitoMevRewards
            && source.connector.as_ref() == "jito-api"
            && source.precedence == 1
    }));
    assert_eq!(requests.load(Ordering::SeqCst), 1);

    rpc_server.abort();
    jito_server.abort();
    Ok(())
}

#[tokio::test]
async fn disabled_economics_mode_does_not_request_jito_rewards() -> Result<()> {
    let (rpc_url, rpc_server) = start_mock_rpc_server().await?;
    let (api_url, jito_server, requests) = start_mock_jito_server(StatusCode::OK, "[]").await?;
    let config = snapshot_config(rpc_url, api_url, false);
    let sources = SourceChain::from_config(&config, RuntimeTelemetry::unobserved())?;

    let snapshot = collect_snapshot(&sources, &config).await?;
    assert_eq!(snapshot.economics, None);
    assert!(!snapshot
        .data_sources
        .iter()
        .any(|source| source.command == CommandId::JitoMevRewards));
    assert_eq!(requests.load(Ordering::SeqCst), 0);

    rpc_server.abort();
    jito_server.abort();
    Ok(())
}

fn snapshot_config(rpc_url: String, provider_api_url: String, economics_enabled: bool) -> Config {
    let mut modes = super::modes_for(&["rpc"], economics_enabled);
    if economics_enabled {
        modes.economics.commands.jito_mev_rewards = vec!["jito-api".to_string()];
    }
    Config {
        network: "jito-test".to_string(),
        connectors: vec![
            ConnectorConfig {
                name: "rpc".to_string(),
                provider: "mock-solana".to_string(),
                features: vec![ProviderFeature::HistoricalData],
                solana_rpc_compatibility: Some(SolanaRpcCompatibility::Extended),
                rpc: Some(crate::config::RpcTransportConfig::new(rpc_url)),
                ..ConnectorConfig::default()
            },
            jito_connector("jito-api", "jito", provider_api_url),
        ],
        modes,
        ..Config::default()
    }
}

fn jito_connector(name: &str, provider: &str, provider_api_url: String) -> ConnectorConfig {
    ConnectorConfig {
        name: name.to_string(),
        provider: provider.to_string(),
        features: vec![ProviderFeature::JitoMevRewards],
        provider_api: Some(crate::config::HttpTransportConfig::new(provider_api_url)),
        ..ConnectorConfig::default()
    }
}

async fn start_mock_jito_server(
    status: StatusCode,
    body: &'static str,
) -> Result<(String, JoinHandle<()>, Arc<AtomicUsize>)> {
    let (url, task, requests, _) = start_header_recording_mock_jito_server(status, body).await?;
    Ok((url, task, requests))
}

type HeaderRecordingMockJitoServer = (
    String,
    JoinHandle<()>,
    Arc<AtomicUsize>,
    Arc<Mutex<Vec<Option<String>>>>,
);

async fn start_header_recording_mock_jito_server(
    status: StatusCode,
    body: &'static str,
) -> Result<HeaderRecordingMockJitoServer> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind mock Jito API listener")?;
    let address = listener
        .local_addr()
        .context("failed to read mock Jito API address")?;
    let requests = Arc::new(AtomicUsize::new(0));
    let authorization_headers = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/api/v1/validators/{vote_account}", get(mock_jito_handler))
        .with_state(MockJitoState {
            status,
            body,
            requests: Arc::clone(&requests),
            authorization_headers: Arc::clone(&authorization_headers),
        });
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    Ok((
        format!("http://{address}/api/v1"),
        task,
        requests,
        authorization_headers,
    ))
}

async fn start_delayed_mock_jito_server() -> Result<(String, JoinHandle<()>, oneshot::Receiver<()>)>
{
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind delayed mock Jito API listener")?;
    let address = listener
        .local_addr()
        .context("failed to read delayed mock Jito API address")?;
    let (request_started, request_received) = oneshot::channel();
    let app = Router::new()
        .route(
            "/api/v1/validators/{vote_account}",
            get(delayed_mock_jito_handler),
        )
        .with_state(DelayedMockJitoState {
            request_received: Arc::new(Mutex::new(Some(request_started))),
        });
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    Ok((format!("http://{address}/api/v1"), task, request_received))
}

async fn delayed_mock_jito_handler(
    State(state): State<DelayedMockJitoState>,
    Path(_vote_account): Path<String>,
) -> impl IntoResponse {
    if let Some(request_started) = state.request_received.lock().await.take() {
        let _ = request_started.send(());
    }
    sleep(Duration::from_secs(3)).await;
    (StatusCode::OK, [("content-type", "application/json")], "[]")
}

async fn mock_jito_handler(
    State(state): State<MockJitoState>,
    headers: HeaderMap,
    Path(_vote_account): Path<String>,
) -> impl IntoResponse {
    state.requests.fetch_add(1, Ordering::SeqCst);
    state.authorization_headers.lock().await.push(
        headers
            .get(reqwest::header::AUTHORIZATION.as_str())
            .and_then(|value| value.to_str().ok())
            .map(str::to_string),
    );
    (
        state.status,
        [("content-type", "application/json")],
        state.body,
    )
}
