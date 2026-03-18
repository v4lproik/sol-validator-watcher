use anyhow::{Context, Result};
use std::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;

#[derive(Clone, Debug)]
pub struct WebsocketProbeClient {
    url: String,
    timeout: Duration,
}

impl WebsocketProbeClient {
    pub fn new(url: String, timeout: Duration) -> Self {
        Self { url, timeout }
    }

    pub async fn probe(&self) -> Result<()> {
        let connect_future = connect_async(self.url.as_str());
        let (stream, _) = timeout(self.timeout, connect_future)
            .await
            .with_context(|| format!("websocket probe to {} timed out", self.url))?
            .with_context(|| format!("failed to connect websocket {}", self.url))?;

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
        assert!(err
            .to_string()
            .contains("failed to connect websocket not-a-websocket-url"));
        Ok(())
    }
}
