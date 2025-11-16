use mycelium_protocol::{impl_message, Message};
/// Generic managed service example - lifecycle management
///
/// Demonstrates ManagedService trait, BoundedPublisher, and OrderedSubscriber
/// using generic domain-agnostic messages.
///
/// Run with: cargo run --example managed_service
use mycelium_transport::{
    BoundedPublisher, BoundedSubscriber, HealthStatus, ManagedService, MessageBus,
    OrderedSubscriber,
};
use std::sync::Arc;
use zerocopy::{AsBytes, FromBytes, FromZeroes};

/// Generic event with sequence number for ordering
#[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
struct DataEvent {
    source_id: u64,
    value: i64,
    sequence: u64,
    timestamp_ms: u64,
}

impl_message!(DataEvent, 1, "events");

/// Generic alert/signal message
#[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
struct Alert {
    severity: u64,    // 1-4 (info, warning, error, critical) - u64 to avoid padding
    event_count: u64, // Events processed before alert
    value: i64,
}

impl_message!(Alert, 2, "alerts");

/// Example handler demonstrating ManagedService lifecycle
struct DataProcessor {
    alert_publisher: BoundedPublisher<Alert>,
    event_count: u64,
    is_healthy: bool,
}

#[async_trait::async_trait]
impl ManagedService for DataProcessor {
    async fn initialize(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("🚀 [Processor] Initializing...");
        // In real service: load config, connect to DB, validate dependencies
        self.is_healthy = true;
        println!("✓  [Processor] Initialized successfully\n");
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("\n🛑 [Processor] Shutting down...");
        // In real service: flush data, close connections, save state
        self.is_healthy = false;
        println!("✓  [Processor] Shutdown complete");
        Ok(())
    }

    async fn health(&self) -> HealthStatus {
        if self.is_healthy {
            HealthStatus::Healthy
        } else {
            HealthStatus::Unhealthy {
                reason: mycelium_transport::UnhealthyReason::NotReady,
            }
        }
    }

    async fn on_error(&mut self, error: Box<dyn std::error::Error + Send + Sync>) {
        println!("⚠️  [Processor] Error: {}", error);
        // Custom error handling: circuit breaker, retry logic, etc.
    }
}

impl DataProcessor {
    fn new(alert_publisher: BoundedPublisher<Alert>) -> Self {
        Self {
            alert_publisher,
            event_count: 0,
            is_healthy: false,
        }
    }

    /// Pure business logic - testable without runtime
    async fn process_event(&mut self, event: Arc<DataEvent>) {
        self.event_count += 1;

        println!(
            "📊 [Processor] Event #{}: source={}, value={}, seq={}",
            self.event_count, event.source_id, event.value, event.sequence
        );

        // Example: Alert on every 100th event
        if self.event_count % 100 == 0 {
            let alert = Alert {
                severity: 1, // Info
                event_count: self.event_count,
                value: event.value,
            };

            if let Err(e) = self.alert_publisher.publish(alert).await {
                println!("⚠️  [Processor] Failed to publish alert: {}", e);
            } else {
                println!(
                    "🔔 [Processor] Alert published at event #{}",
                    self.event_count
                );
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("\n=== Mycelium: Generic Managed Service Demo ===\n");
    println!("Demonstrates:");
    println!("  1. ManagedService - Lifecycle management");
    println!("  2. BoundedPublisher - Backpressure & flow control");
    println!("  3. OrderedSubscriber - FIFO message ordering\n");

    let bus = MessageBus::new();

    // ============================================================
    // Feature 1: BoundedPublisher - Backpressure
    // ============================================================
    println!("📦 Setting up bounded channel (capacity: 10)");
    let (alert_pub, mut alert_sub): (BoundedPublisher<Alert>, BoundedSubscriber<Alert>) =
        bus.bounded_pair(10);
    println!("   ✓ When full, publisher blocks (prevents memory exhaustion)\n");

    // ============================================================
    // Feature 2: ManagedService - Lifecycle
    // ============================================================
    let mut processor = DataProcessor::new(alert_pub.clone());

    println!("🔧 Initializing managed service...");
    processor.initialize().await?;

    let health = processor.health().await;
    println!("   Health status: {}\n", health);

    // ============================================================
    // Feature 3: OrderedSubscriber - FIFO ordering
    // ============================================================
    println!("📋 Setting up ordered event stream");
    let event_sub = bus.subscriber::<DataEvent>();
    let mut ordered_events = OrderedSubscriber::new(event_sub, 0);
    println!("   ✓ Messages delivered in sequence order\n");

    // ============================================================
    // Spawn consumers
    // ============================================================

    // Alert consumer (shows backpressure)
    tokio::spawn(async move {
        while let Some(alert) = alert_sub.recv().await {
            let severity = match alert.severity {
                1 => "INFO",
                2 => "WARN",
                3 => "ERROR",
                4 => "CRITICAL",
                _ => "UNKNOWN",
            };
            println!(
                "🔔 [Alert Consumer] {} - Event count: {}",
                severity, alert.event_count
            );
            // Simulate slow consumer to demonstrate backpressure
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
    });

    // ============================================================
    // Generate and process events
    // ============================================================
    println!("🏃 Processing events...\n");

    let event_pub = bus.publisher::<DataEvent>();

    for i in 0..150 {
        let event = DataEvent {
            source_id: (i % 3) + 1, // Rotate through 3 sources
            value: i as i64 * 10,
            sequence: i,
            timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        };

        event_pub.publish(event).await?;

        // Process through ordered subscriber
        if let Some(ordered_event) = ordered_events.recv().await {
            processor.process_event(ordered_event).await;
        }

        // Slow down occasionally
        if i % 50 == 0 && i > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    println!("\n✓  Processed {} events", processor.event_count);

    // Check ordering stats
    let stats = ordered_events.stats();
    println!("📊 Ordering stats: {}", stats);

    // ============================================================
    // Graceful shutdown
    // ============================================================
    processor.shutdown().await?;

    let final_health = processor.health().await;
    println!("   Final health: {}\n", final_health);

    println!("=== Demo Complete ===\n");
    println!("Key Takeaways:");
    println!("  • ManagedService provides lifecycle hooks (init/shutdown/health)");
    println!("  • BoundedPublisher prevents memory exhaustion via backpressure");
    println!("  • OrderedSubscriber ensures FIFO delivery when needed");
    println!("\n✨ All features are domain-agnostic - works for any use case!");

    Ok(())
}
