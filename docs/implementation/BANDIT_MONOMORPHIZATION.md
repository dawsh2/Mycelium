# Bandit Monomorphization: High-Performance Monolith Mode

**Created:** 2025-11-04  
**Status:** Implementation Guide  
**Prerequisite:** Mycelium monomorphization feature (Phase 4)  
**Target:** Bandit monolith deployment optimization

---

## Executive Summary

**What:** Configure Bandit to use Mycelium's compile-time routing for 30x faster message passing in monolith deployments.

**Why:** Reduce message passing overhead from 65ns (Arc-based) to 2-3ns (direct calls) for latency-critical trading operations.

**Impact:**
- **Order execution latency:** 4ns (2 handlers) vs 130ns (Arc mode)
- **Throughput:** Process 30x more messages/sec with same CPU
- **No code changes:** Services work in both monolith and distributed modes

---

## Performance Comparison

### Current: Arc-Based MessageBus

```rust
// PolygonAdapter emits swap
ctx.emit(swap).await?;  // 65ns overhead

// ArbitrageDetector receives
let swap = subscriber.recv().await?;  // +65ns overhead

// Total: 130ns overhead before any business logic
```

**For 10,000 swaps/second:**
- Overhead: 130ns × 10,000 = 1.3ms/sec
- Impact: 0.13% CPU time (acceptable)

**For 1,000,000 swaps/second:**
- Overhead: 130ns × 1,000,000 = 130ms/sec
- Impact: 13% CPU time (significant!)

---

### Proposed: Compile-Time MonolithBus

```rust
// Direct function calls
bus.emit_v2_swap(&swap);  // 2ns per handler

// Total: 4ns for 2 handlers (ArbitrageDetector, RiskManager)
```

**For 1,000,000 swaps/second:**
- Overhead: 4ns × 1,000,000 = 4ms/sec
- Impact: 0.4% CPU time

**Speedup: 32x faster (130ns → 4ns)**

---

## When To Use Each Mode

### Use MonolithBus (Fast Mode)

**Scenarios:**
- ✅ All services bundled in one binary (development, testing, small-scale production)
- ✅ Latency budget <10μs (order execution, risk checks)
- ✅ High throughput (>100k messages/sec)
- ✅ Single machine deployment

**Performance:**
- 2-3ns per handler
- Can process millions of messages/sec
- Zero allocation overhead

---

### Use MessageBus (Flexible Mode)

**Scenarios:**
- ✅ Services distributed across machines (production scaling)
- ✅ Latency budget >10μs (network dominates)
- ✅ Need runtime service discovery
- ✅ Want deployment flexibility without recompile

**Performance:**
- 65ns per emit (Arc mode)
- 10,000ns per emit (TCP mode)
- Still excellent for most use cases

---

## Implementation Guide

### Step 1: Define Message Types

```rust
// bandit/crates/bandit-protocol/src/messages.rs

use mycelium_protocol::impl_message;
use zerocopy::{IntoBytes, FromBytes, FromZeros, Immutable};

#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, FromZeros)]
#[repr(C)]
pub struct V2Swap {
    pub pool_address: [u8; 20],
    pub token0: [u8; 20],
    pub token1: [u8; 20],
    pub amount_in: [u8; 32],   // U256 as bytes
    pub amount_out: [u8; 32],  // U256 as bytes
    pub timestamp_ns: u64,
}

impl_message!(V2Swap, 19, "market_data");

#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, FromZeros)]
#[repr(C)]
pub struct ArbitrageSignal {
    pub opportunity_id: u64,
    pub path: [u8; 100],  // Encoded swap path
    pub estimated_profit: [u8; 32],  // U256 as bytes
    pub deadline_block: u64,
}

impl_message!(ArbitrageSignal, 20, "arbitrage");
```

**Same message types work for both modes!**

---

### Step 2: Implement Message Handlers

```rust
// bandit/crates/polygon-adapter/src/lib.rs

use mycelium::MessageHandler;
use bandit_protocol::V2Swap;

pub struct PolygonAdapter {
    config: PolygonConfig,
    ws_client: WebSocketClient,
}

// For MonolithBus: Implement handler trait
impl MessageHandler<V2Swap> for PolygonAdapter {
    fn handle(&mut self, _swap: &V2Swap) {
        // PolygonAdapter doesn't consume swaps, it produces them
        // Leave empty or use for monitoring
    }
}

// For MessageBus: Use #[service] macro
#[mycelium::service]
impl PolygonAdapter {
    async fn run(&mut self, ctx: ServiceContext) -> Result<()> {
        // Connect to WebSocket
        let mut stream = self.ws_client.connect(&self.config.ws_url).await?;
        
        while let Some(event) = stream.next().await {
            let swap = self.parse_swap(event)?;
            
            // This works in both modes!
            ctx.emit(swap).await?;
        }
        
        Ok(())
    }
}
```

---

```rust
// bandit/crates/arbitrage-service/src/lib.rs

use mycelium::MessageHandler;
use bandit_protocol::{V2Swap, ArbitrageSignal};

pub struct ArbitrageDetector {
    config: ArbitrageConfig,
    opportunities: Vec<ArbitrageSignal>,
}

// For MonolithBus: Implement handler trait
impl MessageHandler<V2Swap> for ArbitrageDetector {
    fn handle(&mut self, swap: &V2Swap) {
        // Business logic - zero-copy, direct memory access
        if let Some(signal) = self.find_opportunity(swap) {
            self.opportunities.push(signal);
        }
    }
}

// For MessageBus: Use #[service] macro
#[mycelium::service]
impl ArbitrageDetector {
    async fn run(&mut self, ctx: ServiceContext) -> Result<()> {
        let mut swaps = ctx.subscribe::<V2Swap>().await?;
        
        while let Some(swap) = swaps.recv().await {
            if let Some(signal) = self.find_opportunity(&swap) {
                ctx.emit(signal).await?;
            }
        }
        
        Ok(())
    }
}
```

---

```rust
// bandit/crates/risk-manager/src/lib.rs

use mycelium::MessageHandler;
use bandit_protocol::V2Swap;

pub struct RiskManager {
    limits: RiskLimits,
    exposure: ExposureTracker,
}

// For MonolithBus: Implement handler trait
impl MessageHandler<V2Swap> for RiskManager {
    fn handle(&mut self, swap: &V2Swap) {
        // Update exposure tracking
        self.exposure.update(swap);
        
        // Check limits (critical path - must be fast!)
        if self.exposure.exceeds_limits(&self.limits) {
            self.trigger_halt();
        }
    }
}
```

---

### Step 3: Configure Routing

```rust
// bandit/src/routing.rs

use mycelium::routing_config;
use bandit_protocol::{V2Swap, ArbitrageSignal, OrderExecution};

// Import all service types
use polygon_adapter::PolygonAdapter;
use arbitrage_service::ArbitrageDetector;
use risk_manager::RiskManager;
use metrics_collector::MetricsCollector;
use execution_service::ExecutionService;
use audit_logger::AuditLogger;

// Define compile-time routing
routing_config! {
    // V2Swap goes to 3 handlers
    V2Swap => [
        ArbitrageDetector,
        RiskManager,
        MetricsCollector,
    ],
    
    // ArbitrageSignal goes to 2 handlers
    ArbitrageSignal => [
        ExecutionService,
        AuditLogger,
    ],
    
    // OrderExecution goes to 1 handler
    OrderExecution => [
        AuditLogger,
    ],
}

// This macro generates MonolithBus struct with:
// - emit_v2_swap(&V2Swap) method
// - emit_arbitrage_signal(&ArbitrageSignal) method
// - emit_order_execution(&OrderExecution) method
```

**Performance per emit:**
- `emit_v2_swap()`: 6ns (3 handlers × 2ns)
- `emit_arbitrage_signal()`: 4ns (2 handlers × 2ns)
- `emit_order_execution()`: 2ns (1 handler × 2ns)

---

### Step 4A: Monolith Deployment (Fast Mode)

```rust
// bandit/src/bin/monolith.rs

use bandit::routing::MonolithBus;
use bandit_protocol::V2Swap;
use polygon_adapter::PolygonAdapter;
use arbitrage_service::ArbitrageDetector;
use risk_manager::RiskManager;
use metrics_collector::MetricsCollector;
use execution_service::ExecutionService;
use audit_logger::AuditLogger;

#[tokio::main]
async fn main() -> Result<()> {
    // Load configs
    let poly_config = load_config("config/polygon.toml")?;
    let arb_config = load_config("config/arbitrage.toml")?;
    let risk_config = load_config("config/risk.toml")?;
    let metrics_config = load_config("config/metrics.toml")?;
    let exec_config = load_config("config/execution.toml")?;
    let audit_config = load_config("config/audit.toml")?;
    
    // Create all services
    let polygon = PolygonAdapter::new(poly_config);
    let arbitrage = ArbitrageDetector::new(arb_config);
    let risk = RiskManager::new(risk_config);
    let metrics = MetricsCollector::new(metrics_config);
    let execution = ExecutionService::new(exec_config);
    let audit = AuditLogger::new(audit_config);
    
    // Create MonolithBus with all handlers
    let mut bus = MonolithBus::new(
        arbitrage,
        risk,
        metrics,
        execution,
        audit,
    );
    
    // Run adapter loop (producer)
    let mut adapter = polygon;
    
    tokio::spawn(async move {
        loop {
            match adapter.recv_swap().await {
                Ok(swap) => {
                    // Direct calls to all handlers - 6ns total!
                    bus.emit_v2_swap(&swap);
                }
                Err(e) => {
                    eprintln!("Adapter error: {}", e);
                    // Handle reconnection
                }
            }
        }
    });
    
    // Wait for shutdown
    tokio::signal::ctrl_c().await?;
    Ok(())
}
```

**Latency breakdown:**
- WebSocket recv: 50,000ns (network I/O)
- Parse swap: 500ns (business logic)
- Emit to handlers: **6ns** (3 handlers)
- Total: 50,506ns (emit is 0.012% of total)

---

### Step 4B: Distributed Deployment (Flexible Mode)

```rust
// bandit/src/bin/distributed.rs

use mycelium::{MessageBus, ServiceRuntime};
use polygon_adapter::PolygonAdapter;
use arbitrage_service::ArbitrageDetector;

#[tokio::main]
async fn main() -> Result<()> {
    let node = env::var("NODE")?;
    
    match node.as_str() {
        "adapters" => run_adapter_node().await,
        "strategies" => run_strategy_node().await,
        _ => bail!("Unknown node: {}", node),
    }
}

async fn run_adapter_node() -> Result<()> {
    // Use MessageBus for distribution
    let bus = MessageBus::with_tcp("10.0.1.10:9000")?;
    let runtime = ServiceRuntime::new(bus);
    
    let config = load_config("config/polygon.toml")?;
    runtime.spawn_service(PolygonAdapter::new(config)).await?;
    
    runtime.wait_for_shutdown().await
}

async fn run_strategy_node() -> Result<()> {
    let bus = MessageBus::with_tcp("10.0.2.20:9001")?;
    bus.connect_to("10.0.1.10:9000").await?;  // Connect to adapters
    
    let runtime = ServiceRuntime::new(bus);
    
    let config = load_config("config/arbitrage.toml")?;
    runtime.spawn_service(ArbitrageDetector::new(config)).await?;
    
    runtime.wait_for_shutdown().await
}
```

**Same service code works in both deployments!**

---

### Step 5: Hybrid Mode (Best of Both)

For maximum performance, use MonolithBus for critical path and MessageBus for infrastructure:

```rust
// bandit/src/bin/hybrid.rs

use bandit::routing::MonolithBus;
use mycelium::{MessageBus, ServiceRuntime};

#[tokio::main]
async fn main() -> Result<()> {
    // Fast path: MonolithBus for critical services
    let mut fast_bus = MonolithBus::new(
        ArbitrageDetector::new(arb_config),
        RiskManager::new(risk_config),
        ExecutionService::new(exec_config),
    );
    
    // Slow path: MessageBus for infrastructure
    let slow_bus = MessageBus::new();
    let runtime = ServiceRuntime::new(slow_bus);
    
    runtime.spawn_service(MetricsCollector::new(metrics_config)).await?;
    runtime.spawn_service(DatabaseLogger::new(db_config)).await?;
    runtime.spawn_service(AuditLogger::new(audit_config)).await?;
    
    // Adapter feeds both buses
    let mut adapter = PolygonAdapter::new(poly_config);
    
    loop {
        let swap = adapter.recv_swap().await?;
        
        // Critical path: direct calls (6ns)
        fast_bus.emit_v2_swap(&swap);
        
        // Infrastructure: async emit (65ns, but non-blocking)
        let slow_bus = slow_bus.clone();
        let swap_copy = swap.clone();
        tokio::spawn(async move {
            let publisher = slow_bus.publisher::<V2Swap>();
            publisher.publish(swap_copy).await;
        });
    }
}
```

**Performance:**
- Critical path: 6ns (ArbitrageDetector, RiskManager, ExecutionService)
- Infrastructure: 65ns async (doesn't block critical path)

---

## Handling Async Services

### Problem: Async Can't Borrow Across Await

```rust
// This doesn't work with MonolithBus:
impl MessageHandler<V2Swap> for DatabaseLogger {
    async fn handle(&mut self, swap: &V2Swap) {
        // ❌ Can't hold &V2Swap across await
        self.db.insert(swap).await?;
    }
}
```

### Solution: Separate Sync and Async Paths

```rust
// bandit/src/routing.rs

routing_config! {
    V2Swap => {
        // Sync handlers - critical path (zero-copy)
        sync: [ArbitrageDetector, RiskManager],
        
        // Async handlers - infrastructure (copies message)
        async: [DatabaseLogger, AuditService],
    },
}
```

**Generated code:**

```rust
impl MonolithBus {
    // Sync path - zero copy, direct calls
    #[inline(always)]
    pub fn emit_v2_swap_sync(&mut self, swap: &V2Swap) {
        self.arbitrage.handle(swap);  // 2ns
        self.risk.handle(swap);       // 2ns
        // Total: 4ns
    }
    
    // Async path - clones message once
    pub async fn emit_v2_swap_async(&mut self, swap: V2Swap) {
        self.db_logger.handle(&swap).await?;  // Slow (database I/O)
        self.audit.handle(&swap).await?;      // Slow (file I/O)
    }
}
```

**Usage:**

```rust
loop {
    let swap = adapter.recv_swap().await?;
    
    // Critical path: sync, zero-copy (4ns)
    bus.emit_v2_swap_sync(&swap);
    
    // Non-critical path: async, clones once
    tokio::spawn({
        let bus = bus.clone();
        let swap = swap.clone();
        async move {
            bus.emit_v2_swap_async(swap).await;
        }
    });
}
```

**Performance:**
- Sync handlers: 4ns (critical path)
- Async handlers: Don't block (spawned in background)
- Clone cost: ~20ns (amortized, one clone for all async handlers)

---

## Testing Strategy

### Unit Tests (Service Logic)

```rust
// Test individual handlers
#[test]
fn test_arbitrage_detector() {
    let mut detector = ArbitrageDetector::new(test_config());
    
    let swap = V2Swap {
        pool_address: test_pool(),
        amount_in: U256::from(1000),
        // ...
    };
    
    // Test handler directly
    detector.handle(&swap);
    
    assert_eq!(detector.opportunities.len(), 1);
}
```

---

### Integration Tests (MonolithBus)

```rust
// Test routing configuration
#[test]
fn test_monolith_routing() {
    let mut bus = MonolithBus::new(
        ArbitrageDetector::new(test_config()),
        RiskManager::new(test_config()),
    );
    
    let swap = create_test_swap();
    
    // Should call both handlers
    bus.emit_v2_swap(&swap);
    
    // Verify both handlers processed it
    assert_eq!(bus.arbitrage.opportunities.len(), 1);
    assert_eq!(bus.risk.exposure.total(), expected_exposure());
}
```

---

### Benchmark Tests

```rust
use criterion::{black_box, criterion_group, criterion_main, Bencher, Criterion};

fn bench_monolith_bus(c: &mut Criterion) {
    let mut bus = MonolithBus::new(
        ArbitrageDetector::new(bench_config()),
        RiskManager::new(bench_config()),
    );
    
    let swap = create_test_swap();
    
    c.bench_function("monolith_emit", |b| {
        b.iter(|| {
            bus.emit_v2_swap(black_box(&swap));
        });
    });
}

fn bench_message_bus(c: &mut Criterion) {
    let bus = MessageBus::new();
    let publisher = bus.publisher::<V2Swap>();
    let swap = create_test_swap();
    
    c.bench_function("message_bus_emit", |b| {
        b.iter(|| {
            publisher.publish(black_box(swap.clone()));
        });
    });
}

criterion_group!(benches, bench_monolith_bus, bench_message_bus);
criterion_main!(benches);
```

**Expected results:**
- `monolith_emit`: ~4-6ns
- `message_bus_emit`: ~60-70ns
- **Speedup: 10-15x**

---

## Migration Checklist

### From MessageBus to MonolithBus

- [ ] **Step 1:** Implement `MessageHandler<M>` trait for each service
- [ ] **Step 2:** Create `routing.rs` with `routing_config!` macro
- [ ] **Step 3:** Update `main.rs` to use `MonolithBus::new()`
- [ ] **Step 4:** Add benchmarks to verify performance gain
- [ ] **Step 5:** Test thoroughly (unit, integration, load tests)
- [ ] **Step 6:** Deploy to staging environment
- [ ] **Step 7:** Monitor latency metrics
- [ ] **Step 8:** Deploy to production

**Estimated effort:** 1-2 days for refactoring, 1 week for testing

---

## Performance Monitoring

### Metrics to Track

```rust
// Add latency tracking
struct LatencyTracker {
    emit_durations: Vec<Duration>,
}

impl LatencyTracker {
    fn record_emit(&mut self, f: impl FnOnce()) {
        let start = Instant::now();
        f();
        self.emit_durations.push(start.elapsed());
    }
    
    fn report(&self) {
        let avg = self.emit_durations.iter().sum::<Duration>() 
                  / self.emit_durations.len() as u32;
        let max = self.emit_durations.iter().max().unwrap();
        
        println!("Emit latency - avg: {:?}, max: {:?}", avg, max);
    }
}
```

**Expected results:**
- **MonolithBus avg:** 4-6ns
- **MonolithBus p99:** 10-15ns (cache misses)
- **MessageBus avg:** 60-70ns
- **MessageBus p99:** 100-150ns

---

## Troubleshooting

### Issue: Handler Not Called

**Symptom:** Service doesn't receive messages

**Debug:**
```rust
impl MessageHandler<V2Swap> for MyService {
    fn handle(&mut self, swap: &V2Swap) {
        println!("Handler called: {:?}", swap);  // Add debug print
        // Business logic
    }
}
```

**Common causes:**
- Handler not listed in `routing_config!`
- Service not added to `MonolithBus::new()`
- Wrong message type

---

### Issue: Compilation Error "Handler Not Found"

**Symptom:** Compiler error about missing handler

**Fix:**
```rust
// Ensure service implements MessageHandler trait
impl MessageHandler<V2Swap> for MyService {
    fn handle(&mut self, swap: &V2Swap) {
        // Implementation
    }
}

// And is listed in routing_config!
routing_config! {
    V2Swap => [MyService],  // ← Must match type name
}
```

---

### Issue: Performance Not Improving

**Symptom:** Benchmarks show <10x speedup

**Checks:**
1. **Compiler optimizations enabled?**
   ```toml
   [profile.release]
   opt-level = 3
   lto = true
   ```

2. **Inlining happening?**
   ```rust
   #[inline(always)]  // Force inlining
   pub fn emit_v2_swap(&mut self, swap: &V2Swap) { ... }
   ```

3. **Running release build?**
   ```bash
   cargo build --release
   cargo bench  # Not cargo test!
   ```

4. **CPU throttling disabled?**
   ```bash
   # Linux
   sudo cpupower frequency-set -g performance
   ```

---

## FAQ

### Q: Can I mix MonolithBus and MessageBus?

**A: Yes!** Use MonolithBus for critical path, MessageBus for infrastructure:

```rust
let fast = MonolithBus::new(critical_services);
let slow = MessageBus::new();

// Critical path
fast.emit_v2_swap(&swap);  // 4ns

// Infrastructure  
slow.publisher::<V2Swap>().publish(swap).await;  // 65ns
```

---

### Q: What if I need to add a service at runtime?

**A: Use MessageBus.** MonolithBus is compile-time only. If you need runtime flexibility, stick with MessageBus or use hybrid mode.

---

### Q: Can I still use `#[service]` macro?

**A: Yes!** The macro still works with MessageBus. For MonolithBus, implement `MessageHandler` trait directly.

---

### Q: Does this work with async services?

**A: Partially.** Use separate sync/async paths in routing_config (see "Handling Async Services" section).

---

### Q: Will my tests break?

**A: No.** Services can implement both `MessageHandler` (for MonolithBus) and use `#[service]` (for MessageBus). Tests continue to work.

---

## Summary

**MonolithBus provides:**
- ✅ 30x faster message passing (65ns → 2ns)
- ✅ Services remain decoupled (trait-based)
- ✅ Can switch to distributed without code changes
- ✅ Compile-time validation (type safety)
- ✅ Zero runtime overhead (everything inlined)

**When to use:**
- Bundled monolith deployment
- Latency budget <10μs
- High throughput (>100k msg/sec)
- Critical path optimization

**Steps to adopt:**
1. Implement `MessageHandler<M>` traits
2. Configure routing with `routing_config!`
3. Update main.rs to use MonolithBus
4. Benchmark to verify speedup
5. Deploy and monitor

**Result:** Sub-10ns message routing for latency-critical trading operations.
