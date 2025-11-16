use mycelium_protocol::{impl_message, Message};
/// Example demonstrating the three new mycelium features:
/// 1. ManagedService - Service lifecycle management
/// 2. BoundedPublisher - Backpressure and flow control
/// 3. OrderedSubscriber - FIFO message ordering
///
/// This example shows how to build a production-ready service with:
/// - Graceful initialization and shutdown
/// - Health checks
/// - Flow control to prevent memory exhaustion
/// - Ordered message processing (when needed)
use mycelium_transport::{
    BoundedPublisher, BoundedSubscriber, HealthStatus, ManagedService, MessageBus,
    OrderedSubscriber,
};
use std::sync::Arc;
use zerocopy::{AsBytes, FromBytes, FromZeroes};

/// Example handler that processes pool state updates
///
/// This demonstrates the handler pattern from ACTORS.md:
/// - Separate business logic from runtime concerns
/// - Implement ManagedService for lifecycle hooks
/// - Can be used in pub/sub mode today, or actor mode later
struct PoolMonitorHandler {
    signal_publisher: BoundedPublisher<ArbitrageSignal>,
    update_count: u64,
    is_healthy: bool,
}

#[async_trait::async_trait]
impl ManagedService for PoolMonitorHandler {
    async fn initialize(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("🚀 Initializing PoolMonitorHandler...");
        // In a real service:
        // - Load configuration
        // - Connect to databases
        // - Initialize caches
        // - Validate dependencies
        self.is_healthy = true;
        println!("✓  PoolMonitorHandler initialized successfully");
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("🛑 Shutting down PoolMonitorHandler...");
        // In a real service:
        // - Flush pending data
        // - Close database connections
        // - Save state to disk
        // - Clean up resources
        self.is_healthy = false;
        println!("✓  PoolMonitorHandler shut down gracefully");
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
        println!("⚠️  Error in handler: {}", error);
        // Custom error handling
        // Could implement circuit breaker, retry logic, etc.
    }
}

impl PoolMonitorHandler {
    fn new(signal_publisher: BoundedPublisher<ArbitrageSignal>) -> Self {
        Self {
            signal_publisher,
            update_count: 0,
            is_healthy: false,
        }
    }

    /// Business logic: process a pool update
    ///
    /// This is pure, testable logic - no runtime concerns
    async fn handle_update(&mut self, update: Arc<PoolStateUpdate>) {
        self.update_count += 1;

        println!(
            "📊 Processing update #{}: Pool {}",
            self.update_count, update.pool_id
        );

        // Example: Detect arbitrage opportunity (simplified)
        if self.update_count % 100 == 0 {
            let signal = ArbitrageSignal {
                opportunity_id: self.update_count,
                pool_id: update.pool_id,
                estimated_profit_usd: 150.0,
                gas_estimate_wei: 21_000,
                deadline_block: update.block_number + 5,
            };

            // Publish signal (with backpressure)
            if let Err(e) = self.signal_publisher.publish(signal).await {
                println!("⚠️  Failed to publish signal: {}", e);
            } else {
                println!("💰 Arbitrage signal published!");
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("\n=== Mycelium: ManagedService, BoundedPublisher & OrderedSubscriber Demo ===\n");

    // Create message bus
    let bus = MessageBus::new();

    // ============================================================
    // Feature 1: BoundedPublisher - Backpressure & Flow Control
    // ============================================================
    println!("📦 Feature 1: BoundedPublisher with backpressure\n");

    // Create bounded channel with capacity 10 (small for demo)
    let (signal_pub, mut signal_sub) = bus.bounded_pair::<ArbitrageSignal>(10);

    println!("   - Created bounded channel with capacity 10");
    println!("   - When full, publisher blocks (backpressure)");
    println!("   - Prevents memory exhaustion from fast producers\n");

    // ============================================================
    // Feature 2: ManagedService - Lifecycle Management
    // ============================================================
    println!("🔧 Feature 2: ManagedService lifecycle hooks\n");

    let mut handler = PoolMonitorHandler::new(signal_pub.clone());

    // Initialize service
    handler.initialize().await?;

    // Check health
    let health = handler.health().await;
    println!("   Health status: {}\n", health);

    // ============================================================
    // Feature 3: OrderedSubscriber - FIFO Message Ordering
    // ============================================================
    println!("📋 Feature 3: OrderedSubscriber (optional ordering)\n");

    // Regular subscriber (no ordering guarantees)
    let regular_sub = bus.subscriber::<PoolStateUpdate>();

    // Ordered subscriber (FIFO via sequence numbers)
    let _ordered_sub = OrderedSubscriber::new(regular_sub, 0);

    println!("   - Regular subscriber: messages delivered as-received");
    println!("   - Ordered subscriber: enforces FIFO via sequence numbers");
    println!("   - Use ordered when sequence matters (e.g., state updates)\n");

    // ============================================================
    // Run the service
    // ============================================================
    println!("🏃 Running service...\n");

    // Spawn signal consumer (simulates downstream consumer)
    tokio::spawn(async move {
        while let Some(signal) = signal_sub.recv().await {
            println!(
                "   → Received arbitrage signal: ${:.2} profit",
                signal.estimated_profit_usd
            );
            // Simulate slow consumer
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
    });

    // Simulate processing updates
    let publisher = bus.publisher::<PoolStateUpdate>();

    for i in 0..150 {
        let update = PoolStateUpdate {
            pool_id: i as u64,
            venue_id: 1, // Uniswap V2
            reserve0: 1_000_000 + i as u64,
            reserve1: 2_000_000 - i as u64,
            block_number: 1000 + i as u64,
        };

        publisher.publish(update.clone()).await?;
        handler.handle_update(Arc::new(update)).await;

        // Slow down to see backpressure in action
        if i % 50 == 0 && i > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    println!("\n✓  Processed {} updates", handler.update_count);

    // ============================================================
    // Graceful shutdown
    // ============================================================
    println!("\n🛑 Initiating graceful shutdown...\n");

    handler.shutdown().await?;

    let final_health = handler.health().await;
    println!("   Final health status: {}", final_health);

    println!("\n=== Demo Complete ===\n");

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
struct PoolStateUpdate {
    pool_id: u64,
    venue_id: u64,
    reserve0: u64,
    reserve1: u64,
    block_number: u64,
}

impl_message!(PoolStateUpdate, 6001, "pool-state");

#[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
struct ArbitrageSignal {
    opportunity_id: u64,
    pool_id: u64,
    estimated_profit_usd: f64,
    gas_estimate_wei: u64,
    deadline_block: u64,
}

impl_message!(ArbitrageSignal, 6002, "arbitrage-signals");
