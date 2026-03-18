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
