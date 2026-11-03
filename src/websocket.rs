//! WebSocket reachability probes.
//!
//! A successful probe proves transport reachability only; RPC method capability
//! is verified independently by the RPC collector.

use anyhow::{Context, Result};
use std::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;

/// Connector-local client for bounded WebSocket connection probes.
#[derive(Clone)]
pub struct WebsocketProbeClient {
    url: String,
    timeout: Duration,
}

impl WebsocketProbeClient {
    /// Creates a probe client for one URL and connection timeout.
    pub fn new(url: String, timeout: Duration) -> Self {
        Self { url, timeout }
    }

    /// Opens and immediately closes a WebSocket connection.
    pub async fn probe(&self) -> Result<()> {
        let connect_future = connect_async(self.url.as_str());
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
    use anyhow::Result;
    use std::time::Duration;

    #[test]
    fn constructor_stores_url_and_timeout() {
        let client =
            WebsocketProbeClient::new("ws://127.0.0.1:8900".to_string(), Duration::from_secs(3));
        assert_eq!(client.url, "ws://127.0.0.1:8900");
        assert_eq!(client.timeout, Duration::from_secs(3));
    }

    #[tokio::test]
    async fn probe_rejects_invalid_url() -> Result<()> {
        let client =
            WebsocketProbeClient::new("not-a-websocket-url".to_string(), Duration::from_secs(1));
        let err = match client.probe().await {
            Ok(()) => anyhow::bail!("probe should fail"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("failed to connect websocket"));
        Ok(())
    }

    #[tokio::test]
    async fn probe_errors_do_not_expose_secret_query_parameters() -> Result<()> {
        let secret = "do-not-expose-this-api-key";
        let client = WebsocketProbeClient::new(
            format!("not-a-websocket-url?api-key={secret}"),
            Duration::from_secs(1),
        );
        let Err(error) = client.probe().await else {
            anyhow::bail!("invalid websocket URL should fail");
        };

        assert!(
            !format!("{error:#}").contains(secret),
            "websocket errors must redact secret URL query parameters"
        );
        Ok(())
    }
}
