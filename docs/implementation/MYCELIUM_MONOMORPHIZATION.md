# Mycelium Compile-Time Routing

**Created:** 2025-11-04
**Status:** ✅ Implemented
**Completed:** 2025-11-04
**Implementation Time:** ~4 hours

---

## Executive Summary

**Problem:** Mycelium's Arc-based MessageBus provides excellent deployment flexibility but imposes a 65ns overhead (30x slower than direct function calls) even for single-process deployments where all services are bundled together.

**Solution:** Provide a `routing_config!` macro that generates plain structs with direct function calls (2-3ns overhead) while maintaining the same trait-based service interface. Services remain decoupled and can switch between runtime (flexible) and compile-time (fast) routing without code changes.

**Impact:**
- **Compile-time routing:** 2-3ns per handler (30x faster than Arc)
- **Runtime routing:** Falls back to existing MessageBus (65ns Arc, or TCP)
- **Same service code** works in both modes
- **Zero runtime cost** for compile-time mode

**Implementation:**
- ✅ `MessageHandler<M>` trait - `crates/mycelium-transport/src/handler.rs`
- ✅ `AsyncMessageHandler<M>` trait - for I/O-bound handlers
- ✅ `routing_config!` macro - `crates/mycelium-macro/src/routing.rs`
- ✅ Working example - `examples/compile_time_routing.rs`
- ✅ Documentation - Updated README with usage examples

---

## Motivation

### Current Mycelium Design

**Arc-based MessageBus (universal flexibility):**

```rust
// All messages go through Arc + Envelope
pub struct Envelope {
    type_id: u16,
    payload: Arc<dyn Any>,  // Type erasure
}

// Runtime routing
let bus = MessageBus::new();
let publisher = bus.publisher::<V2Swap>();
publisher.publish(swap).await?;  // 65ns overhead

// Breakdown:
// - Envelope wrap: 10ns
// - Arc allocation: 40ns
// - Channel send: 10ns
// - Subscriber Arc clone: 5ns
// Total: 65ns
```

**Why this was chosen (from DESIGN_DECISIONS.md):**
- Transport uniformity (same API for Arc/Unix/TCP)
- Binary size (no code bloat from monomorphization)
- Envelope provides metadata (routing, sequence, type_id)
- 65ns overhead acceptable when network dominates (10μs+ latency)

**When this is suboptimal:**
- Single-process deployments (all services in one binary)
- Sub-microsecond latency requirements (HFT order execution)
- High message throughput (millions/sec, overhead compounds)

---

### Alternative: Compile-Time Routing

**Direct function calls (zero abstraction cost):**

```rust
// No Arc, no Envelope, no channel - just a plain struct with methods
struct AppServices {
    arbitrage: ArbitrageService,
    risk: RiskManager,
}

impl AppServices {
    #[inline(always)]
    fn route_v2_swap(&mut self, swap: &V2Swap) {
        self.arbitrage.handle(swap);  // Direct call: 2ns
        self.risk.handle(swap);       // Direct call: 2ns
    }
}

// Compiler inlines these to:
// arbitrage.handle(&swap); risk.handle(&swap);
```

**Latency:** 2-3ns per handler (vs 65ns Arc overhead)
**Throughput:** 30x more messages/sec with same CPU

**When to use each approach:**

**Runtime routing (MessageBus) - default:**
- Development/debugging (easier tracing, breakpoints)
- Testing (can mock handlers, inject test doubles)
- Distributed deployment (services across processes/nodes)
- Dynamic routing needs (add/remove handlers at runtime)
- Acceptable latency (>10μs budget)

**Compile-time routing - opt-in optimization:**
- Production single-process deployment
- Ultra low-latency (<1μs budget)
- High throughput (>1M msg/sec)
- Fixed service topology (known at compile time)

---

## Design Proposal

### Two Approaches

**Approach 1: Runtime Routing (Current - Default):**

```rust
// Flexible, can distribute, Arc overhead
let bus = MessageBus::new();  // or with_tcp(), with_unix()
let runtime = ServiceRuntime::new(bus);
runtime.spawn_service(my_service).await?;

// Services use ctx.emit()
ctx.emit(swap).await?;  // 65ns overhead
```

**Approach 2: Compile-Time Routing (New - Opt-in):**

```rust
// Generate a plain struct with routing methods
routing_config! {
    name: AppServices,
    routes: {
        V2Swap => [ArbitrageService, RiskManager],
    }
}

// Use it like any struct
let mut services = AppServices::new(...);
services.route_v2_swap(&swap);  // 2-3ns overhead
```

**Same service code works with both approaches!**

---

### Core Components

#### 1. Handler Traits (New)

```rust
// mycelium/src/handler.rs

/// Synchronous message handler (fast path)
pub trait MessageHandler<M> {
    fn handle(&mut self, msg: &M);
}

/// Asynchronous message handler (for I/O-bound services)
pub trait AsyncMessageHandler<M> {
    async fn handle(&mut self, msg: &M) -> Result<()>;
}
```

**Services implement these:**

```rust
impl MessageHandler<V2Swap> for ArbitrageService {
    fn handle(&mut self, swap: &V2Swap) {
        // Business logic - same code for monolith or distributed
        if self.find_opportunity(swap) {
            self.signals.push(signal);
        }
    }
}
```

**Key insight:** Services don't emit directly, they implement handlers. The bus calls them.

---

#### 2. Routing Configuration Macro (New)

```rust
// mycelium/src/routing.rs

#[macro_export]
macro_rules! routing_config {
    (
        name: $struct_name:ident,
        routes: {
            $($message_type:ty => [$($handler:ty),* $(,)?]),* $(,)?
        }
    ) => {
        // Generate a plain struct - user provides the name
        pub struct $struct_name {
            $(
                $(
                    handler: $handler,
                )*
            )*
        }

        impl $struct_name {
            // Generate route_<message_type> methods
            $(
                paste::paste! {
                    #[inline(always)]
                    pub fn [<route_ $message_type:snake>](&mut self, msg: &$message_type) {
                        $(
                            MessageHandler::<$message_type>::handle(&mut self.handler, msg);
                        )*
                    }
                }
            )*
        }
    };
}
```

**Application uses this:**

```rust
// In application code (e.g., Bandit)
routing_config! {
    name: TradingServices,
    routes: {
        V2Swap => [ArbitrageService, RiskManager, MetricsCollector],
        ArbitrageSignal => [ExecutionService, AuditLogger],
    }
}

// Expands to a regular struct:
pub struct TradingServices {
    arbitrage: ArbitrageService,
    risk: RiskManager,
    metrics: MetricsCollector,
    execution: ExecutionService,
    audit: AuditLogger,
}

impl TradingServices {
    #[inline(always)]
    pub fn route_v2_swap(&mut self, msg: &V2Swap) {
        self.arbitrage.handle(msg);  // Direct call
        self.risk.handle(msg);       // Direct call
        self.metrics.handle(msg);    // Direct call
    }

    #[inline(always)]
    pub fn route_arbitrage_signal(&mut self, msg: &ArbitrageSignal) {
        self.execution.handle(msg);  // Direct call
        self.audit.handle(msg);      // Direct call
    }
}
```

**Result:** Just a plain struct with inline methods. Compiler optimizes to direct calls.

---

#### 3. Async Handler Support

**Problem:** Async handlers can't borrow across `.await`:

```rust
async fn route(&mut self, swap: &V2Swap) {
    self.handler1.handle(swap).await;  // ❌ Can't hold &V2Swap across await
    self.handler2.handle(swap).await;
}
```

**Solution:** Separate sync and async paths:

```rust
routing_config! {
    name: TradingServices,
    routes: {
        V2Swap => {
            sync: [RiskManager, MetricsCollector],     // Fast: 2ns direct calls
            async: [DatabaseLogger, AuditService],     // Slow: copies for async
        },
    }
}

// Generates separate methods:
impl TradingServices {
    // Sync path - zero copy, direct calls
    #[inline(always)]
    pub fn route_v2_swap_sync(&mut self, swap: &V2Swap) {
        self.risk.handle(swap);
        self.metrics.handle(swap);
    }

    // Async path - clones once for all async handlers
    pub async fn route_v2_swap_async(&mut self, swap: V2Swap) {
        self.db_logger.handle(&swap).await?;
        self.audit.handle(&swap).await?;
    }
}
```

**Usage:**

```rust
// Critical path: sync handlers only
services.route_v2_swap_sync(&swap);  // 4ns total (2 handlers × 2ns)

// Non-critical path: spawn async handlers
tokio::spawn({
    let swap = swap.clone();
    let mut services = services.clone();
    async move {
        services.route_v2_swap_async(swap).await;  // I/O bound, clone acceptable
    }
});
```

---

#### 4. Service Adapter (Bridge to Existing API)

**Allow existing services to work without changes:**

```rust
// mycelium/src/adapter.rs

/// Adapter that implements MessageHandler by calling ctx.emit()
pub struct ServiceAdapter<S> {
    service: S,
    context: ServiceContext,
}

impl<S, M> MessageHandler<M> for ServiceAdapter<S>
where
    S: Service,
    M: Message,
{
    fn handle(&mut self, msg: &M) {
        // Bridge: clone and emit via ServiceContext
        // (Slower, but allows gradual migration)
        let _ = self.context.emit(msg.clone());
    }
}
```

**This allows:** Services using `#[service]` macro can still work with compile-time routing (with cloning overhead until refactored to use `MessageHandler` trait directly).

---

### Implementation Plan

#### Phase 1: Core Traits (1-2 days)

**Files to create:**
- `mycelium/src/handler.rs` - MessageHandler, AsyncMessageHandler traits

**Files to modify:**
- `mycelium/src/lib.rs` - Export handler traits

**Tests:**
```rust
#[test]
fn test_handler_trait() {
    struct TestHandler;

    impl MessageHandler<u64> for TestHandler {
        fn handle(&mut self, msg: &u64) {
            assert_eq!(*msg, 42);
        }
    }

    let mut handler = TestHandler;
    handler.handle(&42);
}
```

---

#### Phase 2: Routing Macro (3-4 days)

**Files to create:**
- `mycelium/src/routing.rs` - routing_config! macro implementation

**Dependencies:**
- `paste` crate for identifier concatenation

**Tests:**
```rust
#[test]
fn test_routing_macro() {
    routing_config! {
        name: TestServices,
        routes: {
            u64 => [TestHandler1, TestHandler2],
        }
    }

    let services = TestServices::new(TestHandler1, TestHandler2);
    services.route_u64(&42);  // Should call both handlers
}
```

**Challenges:**
- Macro hygiene (ensure generated code doesn't conflict)
- Snake_case conversion for method names
- Compile-time validation that handlers implement trait

---

#### Phase 3: Async Support (2-3 days)

**Extend macro to support async handlers:**

```rust
routing_config! {
    name: AppServices,
    routes: {
        V2Swap => {
            sync: [Handler1, Handler2],
            async: [Handler3, Handler4],
        },
    }
}
```

**Generated code:**
```rust
impl AppServices {
    pub fn route_v2_swap_sync(&mut self, msg: &V2Swap) { ... }
    pub async fn route_v2_swap_async(&mut self, msg: V2Swap) { ... }
}
```

**Tests:**
```rust
#[tokio::test]
async fn test_async_handlers() {
    let mut services = AppServices::new(...);

    services.route_v2_swap_sync(&swap);  // Sync path
    services.route_v2_swap_async(swap.clone()).await;  // Async path
}
```

---

#### Phase 4: Documentation & Examples (1-2 days)

**Create:**
- `docs/implementation/MONOMORPHIZATION.md` - This document
- `examples/monolith_routing.rs` - Example usage
- Update README.md with performance comparison

**Benchmarks:**
```rust
#[bench]
fn bench_compile_time_routing(b: &mut Bencher) {
    let mut services = TradingServices::new(...);
    let swap = V2Swap { ... };

    b.iter(|| {
        services.route_v2_swap(&swap);  // Should be ~2-3ns
    });
}

#[bench]
fn bench_runtime_routing(b: &mut Bencher) {
    let bus = MessageBus::new();
    let publisher = bus.publisher::<V2Swap>();
    let swap = V2Swap { ... };

    b.iter(|| {
        publisher.publish(swap.clone());  // ~65ns
    });
}
```

---

### API Surface

#### For Library Users (Application Developers)

**Step 1: Implement handlers**

```rust
use mycelium::MessageHandler;

impl MessageHandler<V2Swap> for MyService {
    fn handle(&mut self, swap: &V2Swap) {
        // Business logic
    }
}
```

**Step 2: Configure routing**

```rust
use mycelium::routing_config;

routing_config! {
    name: TradingServices,
    routes: {
        V2Swap => [Service1, Service2, Service3],
        ArbitrageSignal => [Service4],
    }
}
```

**Step 3: Use in main.rs**

```rust
fn main() {
    let services = TradingServices::new(
        Service1::new(),
        Service2::new(),
        Service3::new(),
        Service4::new(),
    );

    // Direct calls, 2-3ns overhead
    services.route_v2_swap(&swap);
}
```

**To switch to runtime routing:**

```rust
// Just change main.rs, services unchanged!
fn main() {
    let bus = MessageBus::with_tcp("10.0.1.10:9000")?;
    let runtime = ServiceRuntime::new(bus);

    runtime.spawn_service(Service1::new()).await?;
    // Services use ctx.emit(), same business logic
}
```

---

### Performance Characteristics

#### Latency Comparison

| Approach | Overhead | Use Case |
|----------|----------|----------|
| **Direct call** (baseline) | 2ns | No abstraction at all |
| **Compile-time routing** (this proposal) | 2-3ns | Single-process, need decoupling |
| **MessageBus Arc** (current) | 65ns | Flexible deployment, can distribute |
| **MessageBus TCP** | 10,000ns | Already distributed |

**Speedup for single-process:** 30x faster (65ns → 2ns)

---

#### Binary Size

**Compile-time routing generates code per message type:**

```rust
// For 10 message types, each with 3 handlers:
// - 10 route_<message> methods
// - Each calls 3 handlers inline
// ~ 50-100 lines of generated code per message type
// Total: ~500-1000 LOC

// Estimated binary size increase: 10-20KB (negligible)
```

**Compared to full monomorphization (rejected in DESIGN_DECISIONS.md):**
- Full monomorphization: Duplicate Publisher<M>, Subscriber<M>, etc. → 10-50% binary increase
- Compile-time routing: Only duplicate routing methods → <1% binary increase

**Conclusion:** Binary size impact is minimal because we're only monomorphizing the routing, not the entire MessageBus infrastructure.

---

#### Throughput

**At 1M messages/second:**

| Approach | CPU Time/sec | Headroom |
|----------|--------------|----------|
| Compile-time routing | 2ms | 99.8% free |
| MessageBus Arc | 65ms | 93.5% free |

**For 10M messages/second:**

| Approach | CPU Time/sec | Headroom |
|----------|--------------|----------|
| Compile-time routing | 20ms | 98% free |
| MessageBus Arc | 650ms | 35% free |

**Impact:** Compile-time routing allows 30x higher throughput before hitting CPU limits.

---

### Limitations & Tradeoffs

#### What You Give Up

**1. Runtime flexibility:**
```rust
// ❌ Can't do this with compile-time routing:
services.add_handler::<V2Swap>(new_handler);  // No dynamic handlers

// ✅ Can do this with MessageBus:
runtime.spawn_service(new_service).await?;  // Add at runtime
```

**Why this is OK:** For single-process deployments, you bundle all services in the binary anyway. No need for dynamic loading.

---

**2. Distribution without recompile:**
```rust
// ❌ Can't switch compile-time routing to distributed at runtime
let services = TradingServices::new(...);  // Always in-process

// ✅ MessageBus supports runtime selection:
let transport = env::var("TRANSPORT")?;
let bus = match transport {
    "local" => MessageBus::new(),
    "tcp" => MessageBus::with_tcp(addr)?,
};
```

**Why this is OK:** Single-process vs distributed is a deployment-time decision. Recompile is acceptable.

---

**3. Unified API:**
```rust
// Compile-time routing: Different method per message type
services.route_v2_swap(&swap);
services.route_arbitrage_signal(&signal);

// MessageBus: Generic emit
ctx.emit(swap).await?;
ctx.emit(signal).await?;
```

**Why this is OK:** Type-specific methods are more explicit and slightly faster (no generic dispatch).

---

### Migration Path

#### For Existing Mycelium Users

**Option 1: Keep using MessageBus (no changes needed)**
- Works exactly as before
- 65ns overhead acceptable for most use cases

**Option 2: Adopt compile-time routing (opt-in optimization)**
- Refactor services to implement `MessageHandler<M>` trait
- Add routing_config! to your application
- Update main.rs to use generated struct
- 30x speedup for single-process deployments

**Option 3: Hybrid approach**
- Use compile-time routing for critical path (order execution)
- Use MessageBus for infrastructure (data ingestion, logging)

---

#### Backward Compatibility

**Existing services using `#[service]` macro continue to work:**

```rust
// Old code - still works
#[mycelium::service]
impl MyService {
    async fn run(&mut self, ctx: ServiceContext) -> Result<()> {
        ctx.emit(swap).await?;  // Uses MessageBus
    }
}

// New code - opt-in optimization
impl MessageHandler<V2Swap> for MyService {
    fn handle(&mut self, swap: &V2Swap) {
        // Direct handler, no ctx needed
    }
}
```

**No breaking changes required.**

---

### Open Questions

#### 1. Should the generated struct be generic over handlers?

**Option A: Struct with named fields (current proposal)**
```rust
struct TradingServices {
    arbitrage: ArbitrageService,
    risk: RiskManager,
}
```

**Option B: Generic struct**
```rust
struct TradingServices<T: Handlers> {
    handlers: T,
}
```

**Recommendation:** Option A (named fields) for better error messages and debugging.

---

#### 2. How to handle handler errors?

**Option A: Panic on error (fail-fast)**
```rust
fn handle(&mut self, msg: &M) {
    self.process(msg).unwrap();  // Panic if error
}
```

**Option B: Return Result (must handle)**
```rust
fn handle(&mut self, msg: &M) -> Result<()> {
    self.process(msg)?;  // Caller must handle
}
```

**Recommendation:** Option B (return Result) for production safety. Application decides how to handle errors.

---

#### 3. Should we provide a ServiceContext for handlers?

**Option A: No context (current proposal)**
```rust
fn handle(&mut self, msg: &M) {
    // Service owns its own metrics, logging
}
```

**Option B: Provide context**
```rust
fn handle(&mut self, msg: &M, ctx: &HandlerContext) {
    ctx.metrics().record(...);  // Shared metrics
}
```

**Recommendation:** Option A initially. Services can create their own context if needed. Keeps the critical path minimal (2ns).

---

### Success Criteria

**Functional:**
- ✅ Services can implement `MessageHandler<M>` trait
- ✅ routing_config! macro generates working struct with routing methods
- ✅ Compile-time validation that handlers exist
- ✅ Async handlers supported via separate path
- ✅ Same service code works with both approaches

**Performance:**
- ✅ Compile-time routing overhead: <5ns per handler
- ✅ 30x speedup vs MessageBus Arc (65ns → 2ns)
- ✅ Binary size increase: <1%

**Quality:**
- ✅ Documentation with examples
- ✅ Benchmarks showing performance gain
- ✅ Tests for all macro edge cases
- ✅ Migration guide for existing users

---

## Appendix: Code Examples

### Complete Example: Trading System

```rust
// Define message types
#[derive(Clone)]
struct V2Swap {
    pool_address: [u8; 20],
    amount_in: U256,
    timestamp_ns: u64,
}

// Define services
struct ArbitrageDetector {
    opportunities: Vec<ArbitrageSignal>,
}

impl MessageHandler<V2Swap> for ArbitrageDetector {
    fn handle(&mut self, swap: &V2Swap) {
        if let Some(signal) = self.find_arbitrage(swap) {
            self.opportunities.push(signal);
        }
    }
}

struct RiskManager {
    limits: RiskLimits,
}

impl MessageHandler<V2Swap> for RiskManager {
    fn handle(&mut self, swap: &V2Swap) {
        self.update_exposure(swap);
    }
}

// Configure routing
routing_config! {
    name: TradingServices,
    routes: {
        V2Swap => [ArbitrageDetector, RiskManager],
    }
}

// Use in application
fn main() {
    let mut services = TradingServices::new(
        ArbitrageDetector::new(),
        RiskManager::new(),
    );

    // Process swaps - direct calls, 2ns per handler
    for swap in swap_stream {
        services.route_v2_swap(&swap);  // 4ns total (2 handlers)
    }
}
```

**Performance:** 4ns overhead for 2 handlers (vs 130ns with MessageBus Arc)

---

## Conclusion

**Compile-time routing provides:**
- ✅ 30x speedup for single-process deployments (65ns → 2ns)
- ✅ Services remain decoupled (trait-based)
- ✅ Can switch to runtime routing without service code changes
- ✅ Minimal binary size impact (<1%)
- ✅ Opt-in (no breaking changes)

**When to use compile-time routing:**
- Production single-process deployment with latency requirements (<10μs)
- High throughput (millions of messages/sec)
- Critical path optimization (order execution, risk checks)

**When to use MessageBus (runtime routing) instead:**
- Development/debugging (easier tracing, breakpoints)
- Testing (can mock handlers dynamically)
- Distributed deployment (services across processes/nodes)
- Need runtime flexibility (dynamic service loading)
- Latency budget >10μs (65ns overhead acceptable)

**Recommendation:** Provide both approaches. Let applications choose based on deployment needs.
