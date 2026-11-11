//! Ordered source-event publication with an explicit runtime flush barrier.

use sol_validator_watcher_domain::{FailoverTransition, RpcRequestOutcome, SourceEvent};
use std::{fmt, sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot};

/// Error returned when the runtime source-event consumer has stopped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceTelemetryError;

impl fmt::Display for SourceTelemetryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("runtime source-event consumer stopped")
    }
}

impl std::error::Error for SourceTelemetryError {}

/// One ordered delivery consumed by the runtime actor.
#[derive(Debug)]
pub enum SourceTelemetryDelivery {
    /// A source observation.
    Event(SourceEvent),
    /// A barrier acknowledged after every preceding event has been applied.
    Flush(oneshot::Sender<()>),
}

/// Cloneable event publisher shared by source routes and connector clients.
#[derive(Clone, Default)]
pub struct RuntimeTelemetry {
    deliveries: Option<mpsc::UnboundedSender<SourceTelemetryDelivery>>,
}

/// Single-consumer side of the ordered source-event channel.
pub struct SourceTelemetryReceiver {
    deliveries: mpsc::UnboundedReceiver<SourceTelemetryDelivery>,
}

/// Creates one ordered source-event channel for a runtime actor.
pub fn source_telemetry_channel() -> (RuntimeTelemetry, SourceTelemetryReceiver) {
    let (deliveries, receiver) = mpsc::unbounded_channel();
    (
        RuntimeTelemetry {
            deliveries: Some(deliveries),
        },
        SourceTelemetryReceiver {
            deliveries: receiver,
        },
    )
}

impl RuntimeTelemetry {
    /// Creates a telemetry sink that intentionally discards source observations.
    pub const fn unobserved() -> Self {
        Self { deliveries: None }
    }

    /// Binds RPC observations from this publisher to one configured connector.
    pub fn for_connector(&self, connector: &str) -> ConnectorRpcTelemetry {
        ConnectorRpcTelemetry {
            deliveries: self.deliveries.clone(),
            connector: Arc::from(connector),
        }
    }

    /// Binds WebSocket observations from this publisher to one configured connector.
    pub fn for_websocket_connector(&self, connector: &str) -> ConnectorWebsocketTelemetry {
        ConnectorWebsocketTelemetry {
            deliveries: self.deliveries.clone(),
            connector: Arc::from(connector),
        }
    }

    /// Publishes one committed source failover transition.
    pub fn record_failover_transition(
        &self,
        transition: FailoverTransition,
    ) -> Result<(), SourceTelemetryError> {
        publish(
            &self.deliveries,
            SourceEvent::FailoverTransition(transition),
        )
    }

    /// Publishes one received HTTP response status without connector binding.
    pub fn record_response(&self, status: u16) -> Result<(), SourceTelemetryError> {
        publish(&self.deliveries, SourceEvent::RpcResponse { status })
    }

    /// Waits until the runtime actor has applied every event published before this call.
    pub async fn flush(&self) -> Result<(), SourceTelemetryError> {
        let Some(deliveries) = &self.deliveries else {
            return Ok(());
        };
        let (acknowledgment, applied) = oneshot::channel();
        deliveries
            .send(SourceTelemetryDelivery::Flush(acknowledgment))
            .map_err(|_| SourceTelemetryError)?;
        applied.await.map_err(|_| SourceTelemetryError)
    }
}

impl SourceTelemetryReceiver {
    /// Receives the next source event or flush barrier in FIFO order.
    pub async fn recv(&mut self) -> Option<SourceTelemetryDelivery> {
        self.deliveries.recv().await
    }
}

fn publish(
    deliveries: &Option<mpsc::UnboundedSender<SourceTelemetryDelivery>>,
    event: SourceEvent,
) -> Result<(), SourceTelemetryError> {
    let Some(deliveries) = deliveries else {
        return Ok(());
    };
    deliveries
        .send(SourceTelemetryDelivery::Event(event))
        .map_err(|_| SourceTelemetryError)
}

/// Connector-bound RPC event publisher.
#[derive(Clone)]
pub struct ConnectorRpcTelemetry {
    deliveries: Option<mpsc::UnboundedSender<SourceTelemetryDelivery>>,
    connector: Arc<str>,
}

impl ConnectorRpcTelemetry {
    /// Publishes one received HTTP response status.
    pub fn record_response(&self, status: u16) -> Result<(), SourceTelemetryError> {
        publish(&self.deliveries, SourceEvent::RpcResponse { status })
    }

    /// Publishes one timed request and its protocol outcome.
    pub fn record_request(
        &self,
        method: &'static str,
        duration: Duration,
        outcome: RpcRequestOutcome,
    ) -> Result<(), SourceTelemetryError> {
        publish(
            &self.deliveries,
            SourceEvent::RpcRequestCompleted {
                connector: Arc::clone(&self.connector),
                method,
                duration,
                outcome,
            },
        )
    }

    /// Publishes one semantic failure discovered after a valid response was decoded.
    pub fn record_failure(&self, method: &'static str) -> Result<(), SourceTelemetryError> {
        publish(
            &self.deliveries,
            SourceEvent::RpcFailure {
                connector: Arc::clone(&self.connector),
                method,
            },
        )
    }

    /// Publishes bounded finalized-block cache and JSON-RPC method counts.
    pub fn record_block_fee_cache(
        &self,
        hits: u64,
        misses: u64,
        methods_requested: u64,
    ) -> Result<(), SourceTelemetryError> {
        publish(
            &self.deliveries,
            SourceEvent::BlockFeeCacheObserved {
                connector: Arc::clone(&self.connector),
                hits,
                misses,
                methods_requested,
            },
        )
    }
}

/// Connector-bound WebSocket event publisher.
#[derive(Clone)]
pub struct ConnectorWebsocketTelemetry {
    deliveries: Option<mpsc::UnboundedSender<SourceTelemetryDelivery>>,
    connector: Arc<str>,
}

impl ConnectorWebsocketTelemetry {
    /// Publishes one failed WebSocket reachability probe.
    pub fn record_failure(&self) -> Result<(), SourceTelemetryError> {
        publish(
            &self.deliveries,
            SourceEvent::WebsocketProbeFailure {
                connector: Arc::clone(&self.connector),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{source_telemetry_channel, RuntimeTelemetry, SourceTelemetryError};

    #[tokio::test]
    async fn telemetry_reports_stopped_consumers_and_allows_unobserved_use() {
        let (telemetry, receiver) = source_telemetry_channel();
        drop(receiver);

        assert_eq!(telemetry.record_response(200), Err(SourceTelemetryError));
        assert_eq!(telemetry.flush().await, Err(SourceTelemetryError));
        assert_eq!(RuntimeTelemetry::unobserved().flush().await, Ok(()));
    }
}
