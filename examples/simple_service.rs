//! Simple service example with #[service] macro
//!
//! This example demonstrates:
//! - Service lifecycle (startup/run/shutdown)
//! - Structured logging with trace context (ctx.info(), ctx.warn(), ctx.error())
//! - Automatic metrics collection (emits, latency, errors, restarts)
//! - Graceful shutdown handling

use anyhow::Result;
use mycelium_protocol::impl_message;
use mycelium_transport::{service, ServiceContext, ServiceRuntime};
use mycelium_transport::MessageBus;
use std::time::Duration;
use zerocopy::{IntoBytes, FromBytes, FromZeros, Immutable};

/// Example message type - tick events
#[derive(Debug, Clone, Copy, PartialEq, IntoBytes, FromBytes, FromZeros, Immutable)]
#[repr(C)]
struct TickEvent {
    count: u64,
    timestamp_ns: u64,
}

impl_message!(TickEvent, 100, "example.ticks");

/// A simple counter service that demonstrates observability features
struct CounterService {
    count: u64,
    interval: Duration,
    max_count: u64,
}

impl CounterService {
    fn new(interval: Duration, max_count: u64) -> Self {
        Self {
            count: 0,
            interval,
            max_count,
        }
    }
}

#[service]
impl CounterService {
    async fn run(&mut self, ctx: &ServiceContext) -> Result<()> {
        ctx.info("Counter service starting");

        loop {
            if ctx.is_shutting_down() {
                ctx.info("Received shutdown signal");
                break;
            }

            self.count += 1;

            // Demonstrate structured logging with trace context
            ctx.info(&format!(
                "Counter tick: count={}, elapsed_ns={}",
                self.count,
                ctx.now_ns()
            ));

            // Emit a message (this will be tracked in metrics)
            let tick = TickEvent {
                count: self.count,
                timestamp_ns: ctx.now_ns(),
            };
            ctx.emit(tick).await?;

            // Demonstrate different log levels
            if self.count % 5 == 0 {
                ctx.warn(&format!("Milestone reached: {}", self.count));
            }

            // Stop after max_count
            if self.count >= self.max_count {
                ctx.info(&format!("Reached max count ({}), exiting", self.max_count));
                break;
            }

            ctx.sleep(self.interval).await;
        }

        Ok(())
    }

    async fn startup(&mut self) -> Result<()> {
        log::info!(
            "Counter service initialized (will count to {})",
            self.max_count
        );
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        log::info!("Counter service shutting down, final_count={}", self.count);
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing subscriber for structured logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    log::info!("=== Mycelium Service Observability Example ===");
    log::info!("This example demonstrates:");
    log::info!("  - Automatic trace context propagation");
    log::info!("  - Structured logging (ctx.info/warn/error)");
    log::info!("  - Auto-collected metrics (emits, latency, errors)");
    log::info!("");

    // Create service that will count to 10 with 500ms intervals
    let service = CounterService::new(Duration::from_millis(500), 10);

    let bus = MessageBus::new();

    // Create a subscriber for TickEvents (so messages don't fail)
    let mut subscriber = bus.subscriber::<TickEvent>();

    // Spawn a task to consume the tick events
    tokio::spawn(async move {
        while let Some(tick) = subscriber.recv().await {
            log::debug!(
                "Received tick: count={}, timestamp_ns={}",
                tick.count,
                tick.timestamp_ns
            );
        }
    });

    let runtime = ServiceRuntime::new(bus);

    let handle = runtime.spawn_service(service).await?;

    log::info!("Service running. It will automatically stop after 10 ticks.");
    log::info!("Or press Ctrl+C to stop early.");
    log::info!("");

    // Wait for either Ctrl+C or service completion
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            log::info!("Received Ctrl+C, shutting down...");
            runtime.shutdown().await?;
        }
        _ = handle.join() => {
            log::info!("Service completed naturally");
        }
    }

    log::info!("");
    log::info!("=== Metrics Summary (printed above) ===");
    log::info!("Example complete");
    Ok(())
}
