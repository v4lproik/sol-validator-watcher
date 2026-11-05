//! WebSocket reachability probes.
//!
//! A successful probe proves transport reachability only; failed attempts emit
//! connector-bound telemetry without exposing endpoint URLs.

use crate::{
    config::{has_explicit_authority, WebsocketTransportConfig, WEBSOCKET_RESERVED_HEADERS},
    watcher_state::ConnectorWebsocketTelemetry,
};
use anyhow::{bail, Context, Result};
use std::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        client::IntoClientRequest,
        http::{HeaderMap, HeaderName, HeaderValue, Uri},
    },
};

/// Connector-local client for bounded WebSocket connection probes.
#[derive(Clone)]
pub struct WebsocketProbeClient {
    url: Uri,
    headers: HeaderMap,
    timeout: Duration,
    telemetry: ConnectorWebsocketTelemetry,
}

impl WebsocketProbeClient {
    /// Creates a probe client for one validated transport and telemetry sink.
    pub fn try_new(
        transport: &WebsocketTransportConfig,
        telemetry: ConnectorWebsocketTelemetry,
    ) -> Result<Self> {
        if transport.handshake_timeout_sec == 0 {
            bail!("WebSocket handshake timeout must be greater than 0");
        }
        if !has_explicit_authority(&transport.url) {
            bail!("WebSocket URL must use ws:// or wss:// with a host");
        }
        let url = transport
            .url
            .parse::<Uri>()
            .context("invalid WebSocket URL")?;
        if !matches!(url.scheme_str(), Some("ws" | "wss")) || url.host().is_none() {
            bail!("WebSocket URL must use ws:// or wss:// with a host");
        }
        let base_request = url
            .clone()
            .into_client_request()
            .context("failed to build WebSocket upgrade request")?;
        let mut headers = HeaderMap::with_capacity(transport.headers.iter().count());
        for (name, value) in transport.headers.iter() {
            let name = HeaderName::from_bytes(name.trim().as_bytes())
                .context("invalid WebSocket header name")?;
            if WEBSOCKET_RESERVED_HEADERS.contains(&name.as_str())
                || base_request.headers().contains_key(&name)
            {
                bail!(
                    "custom WebSocket headers must not override protocol-owned header {:?}",
                    name.as_str()
                );
            }
            let mut value = value.parse::<HeaderValue>().with_context(|| {
                format!("invalid value for WebSocket header {:?}", name.as_str())
            })?;
            value.set_sensitive(true);
            if headers.insert(name.clone(), value).is_some() {
                bail!("duplicate WebSocket header name {:?}", name.as_str());
            }
        }
        Ok(Self {
            url,
            headers,
            timeout: Duration::from_secs(transport.handshake_timeout_sec),
            telemetry,
        })
    }

    /// Opens and immediately closes a WebSocket connection.
    pub async fn probe(&self) -> Result<()> {
        let result = self.connect().await;
        let telemetry_result = if result.is_err() {
            self.telemetry.record_failure()
        } else {
            Ok(())
        };

        match (result, telemetry_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Ok(()), Err(error)) => Err(error),
            (Err(error), Ok(())) => Err(error),
            (Err(error), Err(telemetry_error)) => Err(error.context(format!(
                "failed to record websocket probe failure: {telemetry_error:#}"
            ))),
        }
    }

    async fn connect(&self) -> Result<()> {
        let mut request = self
            .url
            .clone()
            .into_client_request()
            .context("failed to build WebSocket upgrade request")?;
        request.headers_mut().extend(self.headers.clone());
        let (stream, _) = timeout(self.timeout, connect_async(request))
            .await
            .context("websocket probe timed out")?
            .context("failed to connect websocket")?;

        drop(stream);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::WebsocketProbeClient;
    use crate::watcher_state::{ConnectorWebsocketTelemetry, RpcTelemetry};
    use anyhow::Result;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    fn telemetry() -> ConnectorWebsocketTelemetry {
        RpcTelemetry::unobserved().for_websocket_connector("websocket-test")
    }

    struct HeaderObserver {
        expected: String,
        seen: Arc<AtomicBool>,
    }

    impl tokio_tungstenite::tungstenite::handshake::server::Callback for HeaderObserver {
        fn on_request(
            self,
            request: &tokio_tungstenite::tungstenite::handshake::server::Request,
            response: tokio_tungstenite::tungstenite::handshake::server::Response,
        ) -> std::result::Result<
            tokio_tungstenite::tungstenite::handshake::server::Response,
            tokio_tungstenite::tungstenite::handshake::server::ErrorResponse,
        > {
            self.seen.store(
                request
                    .headers()
                    .get("authorization")
                    .is_some_and(|value| value == self.expected.as_str()),
                Ordering::SeqCst,
            );
            Ok(response)
        }
    }

    #[test]
    fn constructor_rejects_invalid_urls_without_exposing_queries() -> Result<()> {
        let secret = "websocket-constructor-secret";
        for (url, expected) in [
            (
                format!("ftp://websocket.example/?api-key={secret}"),
                "WebSocket URL must use ws:// or wss:// with a host",
            ),
            (
                format!("ws:///socket?api-key={secret}"),
                "WebSocket URL must use ws:// or wss:// with a host",
            ),
        ] {
            let Err(error) = WebsocketProbeClient::try_new(
                &crate::config::WebsocketTransportConfig {
                    url,
                    handshake_timeout_sec: 1,
                    ..crate::config::WebsocketTransportConfig::default()
                },
                telemetry(),
            ) else {
                anyhow::bail!("invalid WebSocket URL should fail during construction");
            };
            let message = format!("{error:#}");
            assert!(message.contains(expected), "{message}");
            assert!(
                !message.contains(secret),
                "WebSocket constructor errors must redact secret URL query parameters"
            );
        }
        Ok(())
    }

    #[test]
    fn constructor_enforces_timeout_and_reserved_header_invariants() -> Result<()> {
        let zero_timeout: crate::config::WebsocketTransportConfig = toml::from_str(
            r#"
url = "wss://websocket.example"
handshake_timeout_sec = 0
"#,
        )?;
        let Err(error) = WebsocketProbeClient::try_new(&zero_timeout, telemetry()) else {
            anyhow::bail!("a zero WebSocket handshake timeout should fail");
        };
        assert!(error
            .to_string()
            .contains("WebSocket handshake timeout must be greater than 0"));

        let reserved_header: crate::config::WebsocketTransportConfig = toml::from_str(
            r#"
url = "wss://websocket.example"
headers = { content-length = "1" }
"#,
        )?;
        let Err(error) = WebsocketProbeClient::try_new(&reserved_header, telemetry()) else {
            anyhow::bail!("a protocol-owned WebSocket header should fail");
        };
        assert!(error.to_string().contains(
            "custom WebSocket headers must not override protocol-owned header \"content-length\""
        ));
        Ok(())
    }

    #[tokio::test]
    async fn probe_applies_custom_headers_without_exposing_credentials() -> Result<()> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let secret = "websocket-header-secret";
        let expected = format!("Bearer {secret}");
        let header_seen = Arc::new(AtomicBool::new(false));
        let server_observer = HeaderObserver {
            expected: expected.clone(),
            seen: Arc::clone(&header_seen),
        };
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await?;
            let stream = tokio_tungstenite::accept_hdr_async(stream, server_observer).await?;
            drop(stream);
            Ok::<(), anyhow::Error>(())
        });
        let transport: crate::config::WebsocketTransportConfig = toml::from_str(&format!(
            r#"
url = "ws://{address}"
handshake_timeout_sec = 5
headers = {{ authorization = "{expected}" }}
"#
        ))?;
        let debug = format!("{transport:?}");
        assert!(debug.contains("authorization"));
        assert!(!debug.contains(secret));
        assert!(!debug.contains(&address.to_string()));
        let client = WebsocketProbeClient::try_new(&transport, telemetry())?;

        client.probe().await?;
        server.await??;

        assert!(header_seen.load(Ordering::SeqCst));
        Ok(())
    }

    #[tokio::test]
    async fn probe_enforces_handshake_timeout() -> Result<()> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await?;
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            Ok::<(), anyhow::Error>(())
        });
        let client = WebsocketProbeClient::try_new(
            &crate::config::WebsocketTransportConfig {
                url: format!("ws://{address}"),
                handshake_timeout_sec: 1,
                ..crate::config::WebsocketTransportConfig::default()
            },
            telemetry(),
        )?;
        let started = std::time::Instant::now();

        let result = client.probe().await;
        server.abort();

        assert!(result.is_err(), "stalled WebSocket handshake must time out");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "handshake deadline did not bound the stalled WebSocket connection"
        );
        Ok(())
    }

    #[tokio::test]
    async fn probe_errors_do_not_expose_secret_query_parameters() -> Result<()> {
        let secret = "do-not-expose-this-api-key";
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        drop(listener);
        let client = WebsocketProbeClient::try_new(
            &crate::config::WebsocketTransportConfig {
                url: format!("ws://{address}/?api-key={secret}"),
                handshake_timeout_sec: 1,
                ..crate::config::WebsocketTransportConfig::default()
            },
            telemetry(),
        )?;
        let Err(error) = client.probe().await else {
            anyhow::bail!("connection refusal should fail the WebSocket probe");
        };

        assert!(
            !format!("{error:#}").contains(secret),
            "websocket errors must redact secret URL query parameters"
        );
        Ok(())
    }
}
