# Design Decisions

This document explains key architectural choices in Mycelium and the rationale behind them.

---

## Envelope Abstraction vs Full Monomorphization

### Decision

Use `Envelope` with `Arc<dyn Any>` payload for all transports, rather than fully monomorphic channels per message type.

### Context

Mycelium has type-safe `Publisher<M>` and `Subscriber<M>` interfaces, but internally uses:

```rust
// Current approach
pub struct Envelope {
    type_id: u16,
    topic: String,
    sequence: Option<u64>,
    destination: Option<Destination>,
    correlation_id: Option<CorrelationId>,
    payload: Arc<dyn Any + Send + Sync>,  // Type-erased
}

// Channel is NOT generic
broadcast::Sender<Envelope>
broadcast::Receiver<Envelope>
```

**Alternative considered:**

```rust
// Fully monomorphic approach
broadcast::Sender<Arc<M>>  // Generic per message type
broadcast::Receiver<Arc<M>>

// No Envelope needed for local transport
```

### Trade-offs

| Aspect | Current (Envelope) | Alternative (Monomorphic) |
|--------|-------------------|---------------------------|
| **Runtime overhead** | 1-5ns downcast per message | 0ns (compile-time types) |
| **Binary size** | Small (one channel impl) | Large (per-message impl) |
| **Code complexity** | Low (uniform abstraction) | Medium (split local/remote) |
| **Metadata** | Built-in (type_id, sequence, routing) | Must add separately |
| **Transport uniformity** | Same Envelope across all transports | Local diverges from remote |
| **Cache locality** | Better (smaller binary) | Worse (code bloat) |

### Rationale

**1. Marginal Performance Gain**

The downcast overhead is **0.5-2.5% of total latency**:

- Local transport latency: ~200ns
- Downcast cost: 1-5ns
- Real bottlenecks: channel contention, syscalls, network

**Trading 1-5ns for architectural benefits is worthwhile.**

**2. Binary Size Matters**

Full monomorphization means every generic type gets duplicated per message:

```rust
// With 10 message types, all of these get 10x duplicated:
Publisher<M>         // 10 copies
Subscriber<M>        // 10 copies
ChannelManager       // 10 copies
BoundedPublisher<M>  // 10 copies
OrderedSubscriber<M> // 10 copies
// Result: 50+ duplicated implementations
```

**Impacts:**
- Larger binary size (10-50% increase with many message types)
- Worse instruction cache locality
- Potentially slower due to cache misses (can offset 5ns savings!)
- Longer compile times

**3. Envelope Provides Semantic Value**

Envelope isn't just for type erasure - it carries essential metadata:

```rust
pub struct Envelope {
    type_id: u16,              // Runtime type identification
    topic: String,             // Routing key
    sequence: Option<u64>,     // Message ordering
    destination: Option<Destination>,      // Actor routing (Phase 1)
    correlation_id: Option<CorrelationId>, // Request/reply (Phase 1)
    payload: Arc<dyn Any>,     // Message payload
}
```

**Use cases:**
- Logging/tracing: Log type_id without knowing M
- Monitoring: Count messages by type_id
- Actor routing: Use destination field for unicast
- Request/reply: Match responses via correlation_id
- Multi-transport: Same abstraction for Local/Unix/TCP

**Without Envelope, you'd reinvent these features.**

**4. Transport Uniformity**

Remote transports (Unix/TCP) **require** Envelope for serialization:

```rust
// Wire format: [type_id: u16][length: u32][payload: bytes]
// Need type_id to deserialize into correct M
```

**With Envelope:**
- Local: `Envelope { payload: Arc<M> }`
- Unix: Serialize/deserialize Envelope
- TCP: Serialize/deserialize Envelope
- **Same abstraction everywhere**

**Without Envelope:**
- Local: `Arc<M>` directly
- Remote: Custom wrapper for serialization
- **Two divergent code paths**
- More testing surface, more bugs

**5. Future-Proofing**

Envelope enables extensions without breaking changes:

- ✅ Add `priority: u8` field → all transports get it
- ✅ Add `timestamp: u64` → works everywhere
- ✅ Actor routing → already have `destination` field

**Fully monomorphic design would need invasive changes for these.**

### When to Reconsider

**This decision should be revisited if:**

1. **Profiling shows downcast is a bottleneck** (>10% of latency)
   - Requires real workload profiling
   - Unlikely given 200ns baseline

2. **Targeting sub-100ns latency**
   - At <100ns, every nanosecond matters
   - Would justify binary size trade-off

3. **Binary size is not a constraint**
   - Embedded systems care about binary size
   - Cloud systems usually don't

4. **Only using Local transport**
   - If never deploying distributed, Envelope is unnecessary
   - But defeats the purpose of topology-based design

### Performance Optimization Priorities

**If you want to improve performance, focus here first:**

1. **Channel capacity tuning** (5-50ns gain)
   - Default 32 may be suboptimal
   - Tune per workload

2. **Remove sequence tracking** if unused (8 bytes + atomic op)
   - Currently optional but still computed
   - Make it truly zero-cost when disabled

3. **Pool Envelopes** (10-20ns gain)
   - Reuse allocations instead of fresh Arc
   - Object pool for Envelope structs

4. **Lock-free channels** (20-100ns gain)
   - Replace tokio broadcast with crossbeam
   - Better under contention

5. **Batch sends** (amortize overhead)
   - Send multiple messages at once
   - Reduces per-message overhead

**These would give >5ns improvements each with less complexity than full monomorphization.**

---

## Runtime Transport Selection (Enum) vs Compile-Time

### Decision

Use runtime enum dispatch for transport selection:

```rust
pub enum AnyPublisher<M> {
    Local(Publisher<M>),
    Unix(UnixPublisher<M>),
    Tcp(TcpPublisher<M>),
}

// Cost: ~1-2 CPU cycles per publish (enum match)
```

**Alternative:** Compile-time selection via generics

### Rationale

**1. Single Binary Deployment**

Same binary works across all deployment modes:

```bash
# Same executable, different configs
./my-service --config dev.toml      # Local only
./my-service --config staging.toml  # Unix sockets
./my-service --config prod.toml     # TCP distributed
```

**Compile-time selection would require:**
```bash
cargo build --features local
cargo build --features unix
cargo build --features tcp
# Three separate binaries to maintain
```

**2. Negligible Overhead**

Enum match cost: ~1-2 CPU cycles (branch prediction)

- Local: 200ns → 201ns (0.5% overhead)
- Unix: 50μs → 50.001μs (0.002% overhead)
- TCP: 500μs → 500.001μs (0.0002% overhead)

**The flexibility is worth 1-2 cycles.**

**3. Standard Rust Pattern**

Enum dispatch is idiomatic Rust for closed variant sets:

```rust
// This IS the optimal pattern for known variants
enum Transport { Local, Unix, Tcp }
```

**Better than:**
- Trait objects (heap allocation + vtable)
- Generics (monomorphization bloat)
- Macros (poor error messages)

---

## YAML Schema + Codegen vs Manual Structs

### Decision

Use `contracts.yaml` + build-time codegen for message definitions.

### Rationale

**1. Single Source of Truth**

Schema defines:
- Message structure (fields, types)
- TLV type IDs
- Validation rules
- Domain organization
- Documentation

**All generated from one place → no drift between code/docs/IDs.**

**2. Validation at Construction**

Generated constructors enforce invariants:

```rust
// Generated from contracts.yaml validation rules
pub fn new(symbol: &str, decimals: u8) -> Result<Self, ValidationError> {
    if symbol.is_empty() {
        return Err(ValidationError::EmptySymbol);
    }
    if decimals == 0 || decimals > 30 {
        return Err(ValidationError::InvalidDecimals(decimals));
    }
    // ... construct message
}
```

**Impossible states are unrepresentable.**

**3. Consistent TLV Type IDs**

Type IDs are centrally managed:

```yaml
# contracts.yaml
InstrumentMeta:
  tlv_type: 18      # Market Data domain (1-19)

ArbitrageSignal:
  tlv_type: 20      # Signal domain (20-39)
```

**No accidental collisions, clear domain boundaries.**

**4. Zerocopy Trait Generation (DeFi-Critical)**

Build script generates manual unsafe impls for zerocopy:

```rust
// Generated for U256 fields (not auto-derivable)
unsafe impl zerocopy::AsBytes for PoolStateUpdate {
    fn only_derive_is_allowed_to_implement_this_trait() {}
}
```

**Why U256?** DeFi amounts exceed u64 range:
- Token amounts: 1e18 base units (18 decimals) × large quantities = >64 bits
- Example: 1,000,000 USDC = 1,000,000 × 10^6 = 1e12 (fits in u64)
- Example: 1,000,000 ETH = 1,000,000 × 10^18 = 1e24 (**requires U256**)
- Uniswap V3 sqrt pricing: `sqrtPriceX96 = sqrt(price) × 2^96` (needs 160+ bits)
- Liquidity values: Can exceed 2^128 in popular pools

**Zerocopy challenge**: U256 has internal structure that prevents auto-derive
```rust
// U256 is [u64; 4] internally but zerocopy can't auto-verify safety
pub struct U256([u64; 4]);  // 256 bits = 4 × 64-bit words
```

**Build script validation**:
```rust
// Verifies at compile time:
// 1. No padding bytes (sizeof == sum of fields)
// 2. repr(C) layout guaranteed
// 3. All fields are AsBytes + FromBytes
const _: fn() = || {
    let _ = std::mem::transmute::<PoolStateUpdate, [u8; EXPECTED_SIZE]>;
    // Compile error if sizes don't match
};
```

**Result**: Safe zerocopy serialization of 256-bit DeFi amounts with compile-time guarantees.

**5. Generated Tests**

Each message gets:
- Round-trip zerocopy test
- Validation test
- Type ID test

**Free test coverage for every message.**

### Trade-offs

**Cons:**
- Requires build.rs complexity
- YAML changes require rebuild
- Less flexible than manual structs

**Pros:**
- Single source of truth
- Impossible to forget validation
- Consistent conventions
- Generated tests

**Verdict: Worth it for type safety and consistency.**

---

## Actor Model vs Async Tasks

### Decision

Use actor model (mailbox-per-entity pattern) for stateful, long-lived services with supervision needs.

### Context

**Alternative approaches considered:**

```rust
// Option 1: Raw tokio::spawn
tokio::spawn(async move {
    loop {
        // Manual supervision logic
        // Manual state management
        // Shared failure domains
    }
});

// Option 2: Async functions in loop
async fn run_service() {
    loop {
        // No isolation
        // Process-level failure
    }
}

// Option 3: Actor model (chosen)
struct ServiceActor { state: HashMap<Key, Value> }
impl Actor for ServiceActor {
    async fn handle(&mut self, msg: Message) {
        // Isolated state
        // Structured supervision
    }
}
```

### Trade-offs

| Aspect | Raw tokio::spawn | Async Functions | Actor Model (Chosen) |
|--------|------------------|-----------------|----------------------|
| **Fault isolation** | Manual, error-prone | None (shared crash) | Automatic per-actor |
| **State management** | Arc<Mutex<T>> everywhere | Global state | Per-actor ownership |
| **Supervision** | Manual per service (N×M) | Process-level only | Declarative policies |
| **Recovery time** | 5-10s (process restart) | 5-10s (process restart) | 10-50ms (actor restart) |
| **Code complexity** | High (boilerplate per service) | Low initially | Medium (framework overhead) |
| **Message routing** | Manual channels | Pub/sub only | Unicast + pub/sub |

### Rationale

**1. Fault Isolation for Multi-Entity Systems**

CEX trading example (future use case):
- 20+ exchange WebSocket connections (Binance, Coinbase, Kraken, etc.)
- Each connection can fail independently
- Binance outage should NOT crash Coinbase adapter

**Without actors:**
```rust
// Crash in Binance handler kills entire process
tokio::spawn(async {
    let binance = connect_binance().await?;  // ❌ ? propagates to top
    let coinbase = connect_coinbase().await?;
    // All eggs in one basket
});
```

**With actors:**
```rust
// Binance actor crashes → supervisor restarts just that actor (10-50ms)
// Coinbase continues running unaffected
system.spawn_supervised(BinanceActor::new(), RestartPolicy::Always).await;
system.spawn_supervised(CoinbaseActor::new(), RestartPolicy::Always).await;
```

**2. Supervision Patterns at Scale**

**Problem**: Manual supervision code grows O(N) per service

```rust
// Without actors: Manual supervision per service
loop {
    match run_binance_adapter().await {
        Err(e) => {
            error!("Binance failed: {}", e);
            sleep(exponential_backoff()).await;
            // Duplicate this logic for 20 services...
        }
    }
}
```

**Solution**: Declarative supervision policies

```rust
// With actors: Single supervision strategy
SupervisionStrategy::Restart {
    max_retries: 5,
    backoff: ExponentialBackoff { base: 1000 },
}
// Applied uniformly across all actors
```

**3. Per-Entity State Management**

**Use case**: 100+ trading strategies, each with isolated PnL/risk limits

**Without actors**: Shared HashMap + contention
```rust
// Global state → lock contention nightmare
let strategies: Arc<Mutex<HashMap<StrategyId, Strategy>>> = Arc::new(Mutex::new(HashMap::new()));

// Every access needs lock
let mut guard = strategies.lock().await;  // ❌ Bottleneck
guard.get_mut(&id).unwrap().update(data);
```

**With actors**: Each strategy owns its state
```rust
// Each StrategyActor owns its state (no mutex needed)
struct StrategyActor {
    pnl: f64,
    positions: Vec<Position>,
    risk_limits: RiskLimits,
}
// Zero contention - actors process messages sequentially
```

**4. Why Not Just Pub/Sub?**

**Pub/sub is for broadcast (1→N)**:
- Market data → 100 strategies (broadcast)
- All strategies receive same swap event

**Actors are for unicast (1→1) with state**:
- Risk manager → specific strategy (targeted message)
- "Strategy #42, reduce position by 50%"

**Mycelium supports both**:
```rust
// Broadcast via pub/sub
ctx.broadcast(PoolUpdate { ... }).await;

// Unicast via actor ref
ctx.send_to(strategy_42_ref, ReducePosition { amount: 0.5 }).await;
```

**5. Implementation: Mailbox = Topic**

**Key insight**: Actor mailboxes ARE topics (single subscriber)

```rust
// Broadcast topic (pub/sub)
Topic: "pool_updates" → [Sub A, Sub B, Sub C]

// Actor mailbox (unicast)
Topic: "actor.0000000000123abc" → [Actor 123 only]
```

**Benefits**:
- Zero new infrastructure (reuses MessageBus)
- True isolation (actor 123's messages never touch actor 456)
- Per-actor backpressure (slow actor doesn't affect others)
- Simple routing (no filtering logic needed)

**Trade-off**: ~200ns overhead vs raw pub/sub (hash lookup + single-subscriber check)

### When to Reconsider

**This decision should be revisited if:**

1. **Only using pub/sub patterns**
   - Pure DeFi arbitrage with global state → actors add complexity
   - All messages are broadcast → pub/sub alone is simpler

2. **Process-level supervision is acceptable**
   - Can tolerate 5-10s recovery on failures
   - Running in Kubernetes with fast pod restarts
   - State can be rebuilt quickly

3. **No per-entity state requirements**
   - Stateless services only
   - All state in external databases
   - No isolation benefit

4. **Performance-critical single service**
   - <100ns latency requirements
   - Actor overhead (200ns) is significant
   - No fault isolation needed

### Current Status

**Phase 2 Complete** (see docs/ACTORS.md):
- ✅ Full actor system implemented
- ✅ Supervision with restart policies
- ✅ Actor runtime with lifecycle hooks
- ✅ Mailbox = topic design validated
- ✅ Examples: `examples/actor_demo.rs`

**Coexists with pub/sub** - services choose actors OR pub/sub based on needs.

---

## Library vs Framework: Mycelium and Bandit Separation

### Decision

**Mycelium = Reusable messaging library** (runtime primitives)  
**Bandit = Trading framework built on Mycelium** (orchestration + business logic)

Clear separation of concerns: library provides transport-agnostic messaging, framework handles service registry and deployment.

### Context

**Problem:** How to make Mycelium reusable across different domains (trading, gaming, analytics) while still providing ergonomic service orchestration?

**Alternative approaches considered:**

```rust
// Option 1: Monolithic framework (rejected)
// Mycelium knows about PolygonAdapter, ArbitrageService, etc.
mycelium::ServiceRegistry::register("PolygonAdapter", ...);
// ❌ Mycelium becomes domain-specific, not reusable

// Option 2: Pure library, no orchestration (rejected)
// Users manually wire everything
let bus = MessageBus::new();
let adapter = PolygonAdapter::new(config);
tokio::spawn(adapter.run());
// ❌ Every project reimplements supervision, registry, deployment

// Option 3: Library + Framework pattern (chosen)
// Mycelium: Generic primitives
// Bandit: Domain-specific orchestration built on Mycelium
```

### Rationale

**1. Clean Separation of Concerns**

**Mycelium responsibilities (library):**
- ✅ TLV codegen from `contracts.yaml` (build-time)
- ✅ Transport abstraction (Arc/Unix/TCP interfaces)
- ✅ Actor runtime primitives (`ActorRuntime`, `ActorRef<T>`)
- ✅ Service wrapper (`#[mycelium::service]` macro)
- ✅ MessageBus (pub/sub + actor mailboxes)
- ✅ ServiceContext (emit, get_actor, metrics, logging)
- ✅ Observability hooks (metrics, tracing, health)

**Bandit responsibilities (framework):**
- ✅ Service implementations (PolygonAdapter, ArbitrageService, etc.)
- ✅ Service registry (type-safe mapping: service name → constructor)
- ✅ Deployment orchestration (reads topology, spawns services)
- ✅ Config loading (business logic configs)
- ✅ Node agent (optional - for multi-node deployments)

**Clear boundary:** Mycelium never knows about trading concepts. Bandit uses Mycelium APIs like any other application.

**2. Reusability Across Domains**

**With this separation:**
```rust
// Trading system (Bandit)
mycelium::service! { impl PolygonAdapter { ... } }

// Multiplayer game server
mycelium::service! { impl PlayerSession { ... } }

// Real-time analytics
mycelium::service! { impl StreamProcessor { ... } }
```

**All use the same Mycelium library.** Only service implementations differ.

**3. Type-Safe Service Registry Pattern**

**Problem:** Topology YAML contains strings (unavoidable), but we want compile-time validation.

**Solution:** Macro-generated registry in application code (Bandit), not library (Mycelium)

```rust
// In Bandit (NOT Mycelium)
mycelium::service_registry! {
    PolygonAdapter => {
        let config = load_config("poly.toml")?;
        PolygonAdapter::new(config)
    },
    ArbitrageService => {
        let config = load_config("arb.toml")?;
        ArbitrageService::new(config)
    },
}
// Expands to type-safe spawn methods with compile-time validation
```

**Benefits:**
- Strings only exist in YAML (unavoidable) and macro invocation (validated)
- Mycelium stays generic (no hardcoded service names)
- Bandit gets type safety (macro checks constructors exist)
- Topology validation at startup (fail fast on unknown services)

**4. Actor-Centric API (Library Primitive)**

**Mycelium provides:**
```rust
// Generic actor runtime
pub struct ActorRuntime {
    pub async fn spawn_actor<A: Actor>(&self, actor: A) -> ActorRef<A>;
    pub async fn get_actor<A: Actor>(&self) -> Option<ActorRef<A>>;
}

// Typed actor references
pub struct ActorRef<A: Actor> {
    pub async fn send(&self, msg: A::Message) -> Result<()>;
}

// Service context (combines pub/sub + actors)
pub struct ServiceContext {
    pub async fn emit<M: Message>(&self, msg: M) -> Result<()>;
    pub async fn get_actor<A: Actor>(&self) -> Result<ActorRef<A>>;
}
```

**Services are deployment-agnostic:**
```rust
#[mycelium::service]
impl PolygonAdapter {
    async fn run(&mut self, ctx: ServiceContext) -> Result<()> {
        // Pub/sub broadcast
        ctx.emit(V2Swap { ... }).await?;
        
        // Actor unicast (if needed)
        let risk_mgr = ctx.get_actor::<RiskManager>().await?;
        risk_mgr.send(CheckPosition { ... }).await?;
        
        Ok(())
    }
}
```

**Transport is determined by topology (Bandit), not code (Mycelium).**

**5. Topology-Driven Deployment (Framework Concern)**

**Topology configuration (Bandit):**
```yaml
# topology.yaml (read by Bandit, not Mycelium)
nodes:
  adapters:
    host: "10.0.1.10:9000"  # TCP
    services:
      - PolygonAdapter  # Type names validated by macro
      - BaseAdapter
  
  strategies:
    host: null  # Local (Arc)
    services:
      - ArbitrageService
  
  execution:
    host: "unix:///tmp/bandit"  # Unix sockets
    services:
      - ExecutionService
```

**Bandit orchestration:**
```rust
// Deployment orchestrator (Bandit, not Mycelium)
pub struct BanditDeployment {
    topology: Topology,
    registry: ServiceRegistry,  // Macro-generated
}

impl BanditDeployment {
    pub async fn run(self, node_name: &str) -> Result<()> {
        // Read topology to determine transport
        let bus = self.topology.create_bus_for_node(node_name)?;
        let runtime = ActorRuntime::new(bus);  // Mycelium primitive
        
        // Spawn services for this node
        for service_name in self.topology.services_on_node(node_name) {
            self.registry.spawn_actor(service_name, &runtime)?;
        }
        
        runtime.run().await
    }
}
```

**Key insight:** Same service code works everywhere. Topology determines transport at deployment time.

### Benefits of This Design

1. **Transport abstraction preserved** - Services never know about Arc/Unix/TCP
2. **Type safety** - `ActorRef<T>`, macro validation, compile-time checks
3. **Clean separation** - Mycelium is reusable, Bandit is trading-specific
4. **Flexibility** - Same code works: single-process, multi-process, distributed
5. **Performance** - Zero-copy shared memory when co-located
6. **Extensibility** - Pluggable transports (RDMA, DPDK in future)
7. **Simplicity** - Minimal API surface in library

### Implementation Status

**Current (as of 2025-01-03):**
- ✅ Actor runtime primitives (`ActorRuntime`, `ActorRef<M>`)
- ✅ Actor trait and context (`Actor`, `ActorContext`)
- ✅ MessageBus with pub/sub
- ✅ Local transport (Arc)
- ⏳ Typed actor discovery (`get_actor::<T>()`) - **planned Phase 3**
- ⏳ ServiceContext unification - **planned Phase 3**
- ⏳ Service registry macro - **planned Phase 3**
- ⏳ Topology configuration - **planned Phase 3**
- ⏳ Multi-transport support (Unix/TCP) - **planned Phase 4**

**Bandit framework:**
- ⏳ Not yet started (will be separate repository)
- Will demonstrate reference implementation of service registry
- Will provide deployment patterns and examples

### When to Reconsider

**This decision should be revisited if:**

1. **Mycelium never gets reused outside trading**
   - If no one builds games/analytics on Mycelium
   - Separation adds complexity without benefit

2. **Service registry becomes too boilerplate-heavy**
   - If every application reimplements same patterns
   - Consider moving common patterns into optional library extension

3. **Type-safe registry proves too complex**
   - Macro maintenance burden too high
   - Consider runtime-only validation with good error messages

4. **Single-domain focus**
   - If Mycelium pivots to trading-only
   - Could merge with Bandit for simpler architecture

**For now:** Keep separation. Enables reuse, maintains clean boundaries, follows library-not-framework principle.

---

## Summary

**Core philosophy:** Optimize for correctness, maintainability, and flexibility first. Micro-optimize only when profiling shows bottlenecks.

**Key decisions:**
1. **Envelope abstraction** → Worth 1-5ns for metadata, uniformity, and smaller binaries
2. **Runtime transport selection** → Worth 1-2 cycles for single-binary deployment
3. **Schema-driven codegen** → Worth build complexity for type safety and consistency
4. **Actor model** → Worth complexity for fault isolation and supervision at scale
5. **Library/framework separation** → Mycelium = primitives, Bandit = orchestration

**Performance optimization order:**
1. Channel tuning
2. Remove unused features (sequence tracking)
3. Object pooling
4. Lock-free channels
5. Batch operations

**Only then** consider architectural changes like full monomorphization.

---

**Last Updated:** 2025-11-03
**Reviewers:** @daws
