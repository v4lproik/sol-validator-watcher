//! WebSocket reachability probes.
//!
//! A successful probe proves transport reachability only; failed attempts emit
//! connector-bound telemetry without exposing endpoint URLs.

use crate::watcher_state::ConnectorWebsocketTelemetry;
use anyhow::{bail, Context, Result};
use std::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::http::Uri};

/// Connector-local client for bounded WebSocket connection probes.
#[derive(Clone)]
pub struct WebsocketProbeClient {
    url: Uri,
    timeout: Duration,
    telemetry: ConnectorWebsocketTelemetry,
}

impl WebsocketProbeClient {
    /// Creates a probe client for one validated URL, timeout, and telemetry sink.
    pub fn try_new(
        url: &str,
        timeout: Duration,
        telemetry: ConnectorWebsocketTelemetry,
    ) -> Result<Self> {
        let url = url.parse::<Uri>().context("invalid WebSocket URL")?;
        if !matches!(url.scheme_str(), Some("ws" | "wss")) || url.host().is_none() {
            bail!("WebSocket URL must use ws:// or wss:// with a host");
        }
        Ok(Self {
            url,
            timeout,
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
        let connect_future = connect_async(&self.url);
        let (stream, _) = timeout(self.timeout, connect_future)
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
    use std::time::Duration;

    fn telemetry() -> ConnectorWebsocketTelemetry {
        RpcTelemetry::unobserved().for_websocket_connector("websocket-test")
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
                "invalid WebSocket URL",
            ),
        ] {
            let Err(error) =
                WebsocketProbeClient::try_new(&url, Duration::from_secs(1), telemetry())
            else {
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

    #[tokio::test]
    async fn probe_errors_do_not_expose_secret_query_parameters() -> Result<()> {
        let secret = "do-not-expose-this-api-key";
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        drop(listener);
        let client = WebsocketProbeClient::try_new(
            &format!("ws://{address}/?api-key={secret}"),
            Duration::from_secs(1),
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
