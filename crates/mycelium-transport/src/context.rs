//! ServiceContext - The infrastructure API provided to services

use crate::metrics::ServiceMetrics;
use mycelium_protocol::Message;
use mycelium_transport::MessageBus;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::Span;

/// ServiceContext provides infrastructure APIs to services
///
/// This is the main interface services use to interact with Mycelium.
/// Services receive this in their `run(&mut self, ctx: ServiceContext)` method.
pub struct ServiceContext {
    /// Message bus for publishing
    bus: Arc<MessageBus>,

    /// Trace ID for distributed tracing
    trace_id: String,

    /// Current tracing span
    span: Span,

    /// Monotonic start time for latency measurements
    start_time: Instant,

    /// Shutdown signal
    shutdown: broadcast::Receiver<()>,

    /// Metrics collector
    metrics: ServiceMetrics,
}

impl ServiceContext {
    /// Create a new ServiceContext
    pub(crate) fn new(
        bus: Arc<MessageBus>,
        trace_id: String,
        span: Span,
        shutdown: broadcast::Receiver<()>,
        metrics: ServiceMetrics,
    ) -> Self {
        Self {
            bus,
            trace_id,
            span,
            start_time: Instant::now(),
            shutdown,
            metrics,
        }
    }

    /// Emit a message downstream
    pub async fn emit<T: Message>(&self, msg: T) -> anyhow::Result<()> {
        let start = Instant::now();

        tracing::debug!(
            trace_id = %self.trace_id,
            type_id = T::TYPE_ID,
            topic = T::TOPIC,
            "Emitting message"
        );

        // Get publisher for this message type
        let publisher = self.bus.publisher::<T>();

        // Publish the message
        publisher.publish(msg).await?;

        let latency = start.elapsed();
        let latency_us = latency.as_micros() as u64;

        // Record metrics
        self.metrics.record_emit(latency_us);

        tracing::trace!(
            trace_id = %self.trace_id,
            topic = T::TOPIC,
            latency_us = latency_us,
            "Message emitted"
        );

        Ok(())
    }

    /// Get the current trace ID for distributed tracing
    pub fn trace_id(&self) -> &str {
        &self.trace_id
    }

    /// Get the current tracing span
    pub fn span(&self) -> &Span {
        &self.span
    }

    /// Get monotonic timestamp in nanoseconds
    pub fn now_ns(&self) -> u64 {
        self.start_time.elapsed().as_nanos() as u64
    }

    /// Check if the service is shutting down
    pub fn is_shutting_down(&self) -> bool {
        !self.shutdown.is_empty()
    }

    /// Sleep for a duration (cancellable on shutdown)
    pub async fn sleep(&self, duration: Duration) {
        let mut shutdown = self.shutdown.resubscribe();

        tokio::select! {
            _ = tokio::time::sleep(duration) => {},
            _ = shutdown.recv() => {},
        }
    }

    /// Log an informational message with trace context
    pub fn info(&self, msg: &str) {
        tracing::info!(
            trace_id = %self.trace_id,
            "{}",
            msg
        );
    }

    /// Log a warning message with trace context
    pub fn warn(&self, msg: &str) {
        tracing::warn!(
            trace_id = %self.trace_id,
            "{}",
            msg
        );
    }

    /// Log an error message with trace context
    pub fn error(&self, msg: &str) {
        tracing::error!(
            trace_id = %self.trace_id,
            "{}",
            msg
        );

        // Record error metric
        self.metrics.record_error();
    }

    /// Get access to metrics (for custom metrics recording)
    pub fn metrics(&self) -> &ServiceMetrics {
        &self.metrics
    }

    /// Subscribe to messages of a specific type
    ///
    /// This creates a new subscriber that will receive all messages published
    /// to the topic for message type M. The subscriber uses broadcast semantics,
    /// so multiple subscribers will each receive all messages.
    ///
    /// This is the receive counterpart to `emit()`, allowing services to both
    /// send and receive messages through the ServiceContext API.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use mycelium_transport::{ServiceContext, service};
    /// # use mycelium_protocol::{Message, impl_message};
    /// # use zerocopy::{IntoBytes, FromBytes, FromZeros, Immutable};
    /// # use anyhow::Result;
    /// #
    /// # #[derive(Debug, Clone, Copy, IntoBytes, FromBytes, FromZeros, Immutable)]
    /// # #[repr(C)]
    /// # struct PingMessage { id: u64 }
    /// # impl_message!(PingMessage, 1, "ping");
    /// #
    /// # #[derive(Debug, Clone, Copy, IntoBytes, FromBytes, FromZeros, Immutable)]
    /// # #[repr(C)]
    /// # struct PongMessage { id: u64 }
    /// # impl_message!(PongMessage, 2, "pong");
    /// #
    /// # struct PongService;
    /// #
    /// #[service]
    /// impl PongService {
    ///     async fn run(&mut self, ctx: &ServiceContext) -> Result<()> {
    ///         // Create subscriber directly from context
    ///         let mut ping_sub = ctx.subscribe::<PingMessage>();
    ///
    ///         // Receive and respond to messages
    ///         while let Some(ping) = ping_sub.recv().await {
    ///             let pong = PongMessage { id: ping.id };
    ///             ctx.emit(pong).await?;
    ///         }
    ///         Ok(())
    ///     }
    /// }
    /// ```
    pub fn subscribe<M: Message>(&self) -> mycelium_transport::Subscriber<M> {
        self.bus.subscriber()
    }
}
