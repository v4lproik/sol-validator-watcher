use super::{modes_for, start_mock_rpc_server};
use crate::{
    collector::{collect_snapshot, ensure_economics_supported, OperatingCostsClient},
    config::{Config, ConnectorConfig, HttpTransportConfig},
    mode::{CommandId, ProviderFeature},
    source::SourceChain,
    RuntimeTelemetry,
};
use anyhow::{Context, Result};
use axum::{
    extract::{Query, State},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Router,
};
use std::{collections::HashMap, sync::Arc};
use tokio::{net::TcpListener, sync::Mutex, task::JoinHandle};

#[derive(Clone)]
struct MockOperatingCostsState {
    status: Arc<Mutex<StatusCode>>,
    body: Arc<Mutex<String>>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
}

#[derive(Debug, Eq, PartialEq)]
struct RecordedRequest {
    epoch: Option<String>,
    authorization: Option<String>,
}

#[test]
fn operating_cost_client_rejects_invalid_endpoints_without_exposing_queries() -> Result<()> {
    let secret = "cost-api-secret";
    for (url, expected) in [
        (
            format!("ftp://costs.example/v1?api-key={secret}"),
            "operating-cost API URL must use http:// or https:// with a host",
        ),
        (
            format!("https://costs.example/v1?api-key={secret}&epoch=8"),
            "operating-cost API URL must not include the request-owned epoch parameter",
        ),
    ] {
        let Err(error) = OperatingCostsClient::try_new(&HttpTransportConfig::new(url)) else {
            anyhow::bail!("invalid operating-cost endpoint should fail");
        };
        let message = format!("{error:#}");
        assert!(message.contains(expected), "{message}");
        assert!(!message.contains(secret));
    }
    Ok(())
}

#[tokio::test]
async fn operating_cost_client_sends_epoch_and_headers_and_validates_payload() -> Result<()> {
    let (url, state, server) = start_mock_operating_costs_server(
        StatusCode::OK,
        r#"{"epoch":8,"costs":[{"name":" hosting ","lamports_per_epoch":5000000000},{"name":"free-tier","lamports_per_epoch":0}]}"#,
    )
    .await?;
    let secret = "cost-header-secret";
    let transport: HttpTransportConfig = toml::from_str(&format!(
        r#"
url = "{url}"
headers = {{ authorization = "Bearer {secret}" }}
"#
    ))?;
    let client = OperatingCostsClient::try_new(&transport)?;

    let costs = client.get_operating_costs(8).await?;
    server.abort();

    assert_eq!(costs.len(), 2);
    assert_eq!(costs[0].name, "hosting");
    assert_eq!(costs[0].lamports_per_epoch, 5_000_000_000);
    assert_eq!(costs[1].name, "free-tier");
    assert_eq!(costs[1].lamports_per_epoch, 0);
    assert_eq!(
        state.requests.lock().await.as_slice(),
        &[RecordedRequest {
            epoch: Some("8".to_string()),
            authorization: Some(format!("Bearer {secret}")),
        }]
    );
    Ok(())
}

#[tokio::test]
async fn operating_cost_client_rejects_incomplete_or_inconsistent_budgets() -> Result<()> {
    let (url, state, server) =
        start_mock_operating_costs_server(StatusCode::OK, r#"{"epoch":7,"costs":[]}"#).await?;
    let client = OperatingCostsClient::try_new(&HttpTransportConfig::new(url))?;
    let too_many_costs = (0..65)
        .map(|index| format!(r#"{{"name":"cost-{index}","lamports_per_epoch":1}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let cases = [
        (
            r#"{"epoch":7,"costs":[]}"#.to_string(),
            "returned epoch 7, requested 8",
        ),
        (
            r#"{"epoch":8,"costs":[]}"#.to_string(),
            "must contain at least one cost",
        ),
        (
            r#"{"epoch":8,"costs":[{"name":" ","lamports_per_epoch":1}]}"#.to_string(),
            "name must not be empty",
        ),
        (
            format!(
                r#"{{"epoch":8,"costs":[{{"name":"{}","lamports_per_epoch":1}}]}}"#,
                "x".repeat(129)
            ),
            "name exceeds 128 bytes",
        ),
        (
            r#"{"epoch":8,"costs":[{"name":"line\nbreak","lamports_per_epoch":1}]}"#.to_string(),
            "name must not contain control characters",
        ),
        (
            r#"{"epoch":8,"costs":[{"name":"hosting","lamports_per_epoch":1},{"name":" hosting ","lamports_per_epoch":2}]}"#.to_string(),
            "duplicate cost name \"hosting\"",
        ),
        (
            r#"{"epoch":8,"costs":[{"name":"hosting","lamports_per_epoch":18446744073709551615},{"name":"rpc","lamports_per_epoch":1}]}"#.to_string(),
            "total exceeds the u64 lamport limit",
        ),
        (
            format!(r#"{{"epoch":8,"costs":[{too_many_costs}]}}"#),
            "contains 65 costs; maximum is 64",
        ),
        (
            r#"{"epoch":8,"costs":[{"name":"hosting","lamports_per_epoch":1}],"unexpected":true}"#
                .to_string(),
            "response was invalid",
        ),
    ];

    for (body, expected) in cases {
        *state.body.lock().await = body;
        let error = client
            .get_operating_costs(8)
            .await
            .err()
            .context("invalid operating-cost response should fail")?;
        assert!(error.to_string().contains(expected), "{error:#}");
    }

    *state.body.lock().await = " ".repeat(1_048_577);
    let error = client
        .get_operating_costs(8)
        .await
        .err()
        .context("oversized operating-cost response should fail")?;
    assert!(error
        .to_string()
        .contains("operating-cost response exceeds 1048576 bytes"));
    server.abort();
    Ok(())
}

#[tokio::test]
async fn operating_cost_collection_falls_back_refreshes_and_fails_closed() -> Result<()> {
    let (rpc_url, rpc_server) = start_mock_rpc_server().await?;
    let (failed_url, failed_state, failed_server) =
        start_mock_operating_costs_server(StatusCode::SERVICE_UNAVAILABLE, "unavailable").await?;
    let (success_url, success_state, success_server) = start_mock_operating_costs_server(
        StatusCode::OK,
        r#"{"epoch":8,"costs":[{"name":"hosting","lamports_per_epoch":5000000000}]}"#,
    )
    .await?;
    let mut config = Config {
        network: "devnet".to_string(),
        connectors: vec![
            ConnectorConfig {
                name: "rpc".to_string(),
                provider: "mock-solana".to_string(),
                features: vec![ProviderFeature::HistoricalData],
                rpc: Some(crate::config::RpcTransportConfig::new(rpc_url)),
                ..ConnectorConfig::default()
            },
            operating_cost_connector("cost-primary", failed_url),
            operating_cost_connector("cost-backup", success_url),
        ],
        modes: modes_for(&["rpc"], true),
        ..Config::default()
    };
    config.modes.economics.commands.operating_costs =
        vec!["cost-primary".to_string(), "cost-backup".to_string()];
    let sources = SourceChain::from_config(&config, RuntimeTelemetry::unobserved())?;
    ensure_economics_supported(&sources, &config).await?;

    let first = collect_snapshot(&sources, &config).await?;
    let economics = first.economics.context("missing economics metrics")?;
    assert_eq!(economics.operating_costs.len(), 1);
    assert_eq!(
        economics.operating_costs[0].lamports_per_epoch,
        5_000_000_000
    );
    assert!(first.data_sources.iter().any(|source| {
        source.command == CommandId::OperatingCosts
            && source.connector.as_ref() == "cost-backup"
            && source.precedence == 2
    }));

    *success_state.body.lock().await =
        r#"{"epoch":8,"costs":[{"name":"hosting","lamports_per_epoch":6000000000},{"name":"rpc","lamports_per_epoch":1000000000}]}"#
            .to_string();
    let second = collect_snapshot(&sources, &config).await?;
    let costs = &second
        .economics
        .context("missing refreshed economics metrics")?
        .operating_costs;
    assert_eq!(costs.len(), 2);
    assert_eq!(costs[0].lamports_per_epoch, 6_000_000_000);
    assert_eq!(costs[1].lamports_per_epoch, 1_000_000_000);

    *success_state.status.lock().await = StatusCode::BAD_GATEWAY;
    let error = collect_snapshot(&sources, &config)
        .await
        .err()
        .context("exhausted operating-cost route should fail the poll")?;
    assert!(error
        .to_string()
        .contains("connector route exhausted for required command operating-costs"));
    assert_eq!(failed_state.requests.lock().await.len(), 3);
    assert_eq!(success_state.requests.lock().await.len(), 3);

    rpc_server.abort();
    failed_server.abort();
    success_server.abort();
    Ok(())
}

fn operating_cost_connector(name: &str, url: String) -> ConnectorConfig {
    ConnectorConfig {
        name: name.to_string(),
        provider: "operator-cost-service".to_string(),
        features: vec![ProviderFeature::OperatingCosts],
        provider_api: Some(HttpTransportConfig::new(url)),
        ..ConnectorConfig::default()
    }
}

async fn start_mock_operating_costs_server(
    status: StatusCode,
    body: &str,
) -> Result<(String, MockOperatingCostsState, JoinHandle<()>)> {
    let state = MockOperatingCostsState {
        status: Arc::new(Mutex::new(status)),
        body: Arc::new(Mutex::new(body.to_string())),
        requests: Arc::new(Mutex::new(Vec::new())),
    };
    let app = Router::new()
        .route("/costs", get(mock_operating_costs_handler))
        .with_state(state.clone());
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind mock operating-cost API")?;
    let address = listener
        .local_addr()
        .context("failed to read mock operating-cost API address")?;
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok((format!("http://{address}/costs"), state, server))
}

async fn mock_operating_costs_handler(
    State(state): State<MockOperatingCostsState>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let request = RecordedRequest {
        epoch: query.get("epoch").cloned(),
        authorization: headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string),
    };
    state.requests.lock().await.push(request);
    (*state.status.lock().await, state.body.lock().await.clone())
}
