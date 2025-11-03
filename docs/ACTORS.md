# Actor System Design for Mycelium

## Executive Summary

This document outlines a **two-phase approach** to build a complete actor system on top of mycelium's pub/sub foundation.

**Key Insight**: An actor's mailbox is just a pub/sub topic with a single, dedicated subscriber. By treating actors as a specialized use case of our existing pub/sub mechanism, we avoid building two separate systems.

**Strategic Vision**: Mycelium is being built as a **general trading framework** supporting DeFi arbitrage, CEX trading, market making, multi-strategy portfolios, and TradFi. Actors are essential for managing complexity at scale across these diverse use cases.

**Status**: Phase 1 (actor-ready foundation) in progress. Phase 2 (full actor system) recommended for near-term implementation.

---

## Table of Contents

1. [Motivation](#motivation)
2. [Actor vs Pub/Sub Trade-offs](#actor-vs-pubsub-trade-offs)
3. [Design Philosophy](#design-philosophy)
4. [Phase 1: Actor-Ready Foundation](#phase-1-actor-ready-foundation)
5. [Phase 2: Full Actor System](#phase-2-full-actor-system)
6. [Handler Pattern](#handler-pattern)
7. [Migration Examples](#migration-examples)
8. [Implementation Details](#implementation-details)
9. [Decision Tree](#decision-tree)

---

## Motivation

### The Problem: Fine-Grained Fault Isolation

**Scenario**: Your `polygon-adapter` service maintains WebSocket connections to three different RPC providers for redundancy.

**With Process-Level Supervision** (systemd/k8s):
```
Provider-1 WebSocket panics
→ Entire process dies
→ Systemd waits 5s, restarts whole process
→ All 3 WebSocket connections drop and reconnect
→ 5-10 second gap in market data
→ Miss arbitrage opportunities during outage
```

**With Actor Supervision**:
```
Provider-1 actor panics
→ Supervisor catches it
→ Restarts ONLY provider-1 actor in ~10-50ms
→ Providers 2 & 3 never interrupted
→ Continuous market data from 2/3 providers
```

**Impact**: 100-1000x faster recovery time for isolated failures.

### Additional Benefits

| Feature | Pub/Sub | Actors | Use Case |
|---------|---------|--------|----------|
| **Broadcast** | ✅ Native | ⚠️ Inefficient | Market data distribution |
| **Stateful per-entity** | ⚠️ Manual partitioning | ✅ Natural | Per-exchange, per-strategy |
| **Guaranteed ordering** | ❌ FIFO across all | ✅ Per-actor mailbox | Sequential execution |
| **Load balancing** | ⚠️ Manual | ✅ Built-in | Multi-instance services |
| **Fine-grained supervision** | ❌ Process-level | ✅ Actor-level | Fault isolation |
| **Complexity at scale** | ⚠️ Linear growth | ✅ Sub-linear | 100+ services |

---

## Actor vs Pub/Sub Trade-offs

### When Pub/Sub Wins

**Use cases**:
- Broadcasting market data to many consumers
- Event-driven architectures (many subscribers per event)
- Stateless processing
- Simple monitoring/logging

**Example**:
```rust
// Polygon adapter publishes swap events
// → Strategy 1 (arbitrage)
// → Strategy 2 (market making)
// → Dashboard
// → Data logger
```

**Winner**: Pub/sub - this is 90% of message volume in most systems.

---

### When Actors Win

**Use cases**:
- Per-entity state tracking (one actor per exchange, strategy, symbol)
- Independent execution with isolated failure domains
- Complex stateful services with multiple subtasks
- Services with different failure recovery strategies per component
- Scaling to 100+ services/strategies/venues

**Example 1: Multi-Exchange Trading**:
```rust
// Each exchange is an isolated actor
BinanceActor     { connection, orders, rate_limits }
CoinbaseActor    { connection, orders, rate_limits }
KrakenActor      { connection, orders, rate_limits }
// ... 20 more exchanges

// Benefits:
// ✅ Binance failure doesn't affect Coinbase
// ✅ Independent restart policies per exchange
// ✅ Isolated rate limiting
// ✅ Scale from 1 to 100 exchanges with same code
```

**Example 2: Multi-Strategy Portfolio**:
```rust
// Each strategy is an isolated actor
ArbitrageActor      { positions, pnl: $1,234, risk_limits }
MarketMakingActor   { positions, pnl: $5,678, risk_limits }
MomentumActor       { positions, pnl: -$123,  risk_limits }
ValueActor          { positions, pnl: $9,999, risk_limits }
// ... 96 more strategies

// Benefits:
// ✅ Strategy 1 crash doesn't kill other 99 strategies
// ✅ Per-strategy risk isolation
// ✅ Independent PnL tracking
// ✅ Scale from 1 to 1000 strategies
```

**Winner**: Actors - essential for scaling a trading framework beyond single-strategy systems.

---

### Important Nuance: DeFi Arbitrage vs General Trading

**DeFi Arbitrage** is a special case that doesn't benefit from per-pool actors:

```rust
// Arbitrage requires a GLOBAL view of liquidity
// Path: WETH → USDC → DAI → WETH needs to see:
// - Uniswap WETH/USDC pool
// - Sushiswap USDC/DAI pool
// - Curve DAI/WETH pool
// ALL AT ONCE to calculate path profitability

// Centralized state manager is correct
struct PoolStateManager {
    pools: DashMap<Address, PoolState>,  // 10K pools, concurrent access
    token_graph: RwLock<Graph<Token>>,   // Liquidity graph for pathfinding
}
```

**BUT** - Most trading patterns DO benefit from actors:

| Trading Pattern | Natural Partitioning | Actor Model Fits? |
|----------------|---------------------|-------------------|
| **DeFi Arbitrage** | ❌ Global liquidity graph | No (exception) |
| **CEX Trading** | ✅ Per-exchange (Binance, Coinbase, Kraken) | **YES** ✅ |
| **Market Making** | ✅ Per-venue (independent order books) | **YES** ✅ |
| **Multi-Strategy** | ✅ Per-strategy (isolated risk/PnL) | **YES** ✅ |
| **TradFi Equities** | ✅ Per-symbol (different feeds, lifecycle) | **YES** ✅ |
| **Options Trading** | ✅ Per-underlying (greeks, expiry) | **YES** ✅ |
| **Portfolio Mgmt** | ✅ Per-account (positions, limits) | **YES** ✅ |

**Conclusion**: Actors are essential for a general trading framework. DeFi arbitrage uses pub/sub (the exception), everything else uses actors (the rule).

---

### Complexity Growth: With vs Without Actors

**Without Actors** (manual supervision):
```
Services: 1  → 5   → 20   → 100
Code:     100 → 500 → 5000 → ???
```

**Linear code growth** because each service needs:
- Custom supervision logic
- Manual state management
- Explicit error handling
- Coordination primitives

**With Actors**:
```
Services: 1  → 5   → 20   → 100
Code:     150 → 200 → 300 → 500
```

**Sub-linear growth** because:
- Supervision is free (ActorSystem handles it)
- State is isolated (per-actor)
- Error handling centralized (supervision policies)
- Coordination via messages (uniform interface)

**At 100 services, actors reduce code by 10x**.

---

## Design Philosophy

### Core Principles

1. **Pub/sub is the foundation** - Actors are built on top, not alongside
2. **Zero overhead when unused** - No runtime cost for pure pub/sub
3. **Handler pattern** - Separate business logic from runtime
4. **Two patterns, one framework** - Pub/sub for broadcast, actors for isolation
5. **Complexity reduction** - Actors reduce code growth as services scale
6. **Backward compatible** - Existing code works unchanged

### When to Use Each Pattern

**Use Pub/Sub for**:
- Market data distribution (one-to-many broadcast)
- Event streaming (logs, metrics, monitoring)
- Fan-out (single event → multiple consumers)
- Stateless processing
- Global state (DeFi liquidity graphs)

**Use Actors for**:
- Per-exchange/venue isolation (CEX, market making)
- Per-strategy isolation (multi-strategy portfolios)
- Per-symbol tracking (equities, options)
- Fault-tolerant services with multiple failure domains
- Scaling beyond 10+ stateful services

### Key Design Decision: Topic-Based Routing

**Actors use dedicated topics instead of shared topics with filtering**:

```rust
// Broadcast pub/sub: shared topic
Topic: "pool_updates" → [Subscriber A, Subscriber B, Subscriber C]

// Actor mailbox: dedicated topic
Topic: "actor.00000123abc" → [Actor 123 (single subscriber)]
Topic: "actor.00000456def" → [Actor 456 (single subscriber)]
```

**Why**:
- ✅ True isolation (actor 123's messages don't touch actor 456)
- ✅ Backpressure per actor (actor 123 slow doesn't affect actor 456)
- ✅ Simple (no filtering logic)
- ✅ Efficient (no wasted work)

**Trade-off**: More topics, but Rust HashMap is O(1) and handles this easily.

---

## Phase 1: Actor-Ready Foundation

**Goal**: Add ~100 lines of code to enable actors later without any changes to existing code.

### Changes Required

#### 1. Message Trait (Remove Copy Requirement)

```rust
// BEFORE
pub trait Message: AsBytes + FromBytes + Send + Sync + Debug + Copy + Clone

// AFTER (allows messages with heap data)
pub trait Message: AsBytes + FromBytes + Send + Sync + Debug + Clone + 'static {
    const TYPE_ID: u16;
    const TOPIC: &'static str;
}
```

**Rationale**: Messages may need `Vec<T>`, `String`, etc. in the future. Note: We use `FixedVec` currently to avoid heap allocations while maintaining zerocopy compatibility.

---

#### 2. Add Routing Types (Optional Metadata)

```rust
// crates/mycelium-protocol/src/routing.rs (NEW)

/// Routing hint for message delivery (optional, zero-cost if unused)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    Broadcast,           // Default: all subscribers
    Unicast(ActorId),    // Future: specific actor
    Partition(u64),      // Future: hash-based routing
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActorId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CorrelationId(pub u128);
```

**Usage**: None today, available for actors/request-reply later.

---

#### 3. Enhance Envelope (Optional Metadata Fields)

```rust
pub struct Envelope {
    type_id: u16,
    topic: String,
    destination: Option<Destination>,        // NEW: None = broadcast
    correlation_id: Option<CorrelationId>,   // NEW: None = no correlation
    payload: Arc<dyn Any + Send + Sync>,
}

impl Envelope {
    // Standard constructor (current behavior)
    pub fn new<M: Message + 'static>(msg: M, topic: String) -> Self {
        Self {
            type_id: M::TYPE_ID,
            topic,
            destination: None,        // Zero cost
            correlation_id: None,     // Zero cost
            payload: Arc::new(msg),
        }
    }

    // Advanced constructor (future)
    pub fn with_destination<M: Message + 'static>(
        msg: M,
        topic: String,
        destination: Destination
    ) -> Self {
        // ... sets destination metadata
    }
}
```

**Impact**: Existing code uses `Envelope::new()`, gets `None` for both fields (optimized away by compiler).

---

#### 4. Expose Dynamic Topic API

```rust
// crates/mycelium-transport/src/bus.rs

impl MessageBus {
    // Existing: implicit topic
    pub fn publisher<M: Message>(&self) -> Publisher<M> {
        self.publisher_for_topic(M::TOPIC)
    }

    pub fn subscriber<M: Message>(&self) -> Subscriber<M> {
        self.subscriber_for_topic(M::TOPIC)
    }

    // NEW: explicit topic (for actors)
    pub fn publisher_for_topic<M: Message>(&self, topic: &str) -> Publisher<M> {
        let sender = self.local.channel_manager.get_or_create(topic);
        Publisher::new(sender)
    }

    pub fn subscriber_for_topic<M: Message>(&self, topic: &str) -> Subscriber<M> {
        let receiver = self.local.channel_manager.subscribe(topic);
        Subscriber::new(receiver)
    }
}
```

**Usage today**: None (internal implementation detail becomes public API).

---

#### 5. Topic Naming Helpers

```rust
// crates/mycelium-transport/src/topics.rs (NEW)

pub struct TopicBuilder;

impl TopicBuilder {
    /// Standard broadcast topic
    pub fn broadcast(name: &str) -> String {
        name.to_string()
    }

    /// Actor mailbox topic
    pub fn actor_mailbox(actor_id: ActorId) -> String {
        format!("actor.{:016x}", actor_id.0)
    }

    /// Partitioned topic
    pub fn partition(base: &str, partition: usize) -> String {
        format!("{}.{}", base, partition)
    }
}
```

**Usage today**: Optional helpers, not required.

---

### Summary: Phase 1

**Code changes**: ~100 lines
**Breaking changes**: 0
**Runtime overhead**: 0 (all defaults are optimized away)
**Enables**: Full actor system in Phase 2 without touching Phase 1 code

---

## Phase 2: Full Actor System

**Goal**: Build `mycelium-actors` crate to enable CEX trading, market making, multi-strategy portfolios, and TradFi.

**When to build**: **Recommended for near-term** given the goal of building a general trading framework beyond DeFi arbitrage.

### Actor System API

```rust
// crates/mycelium-actors/src/lib.rs (NEW CRATE)

pub trait Actor {
    type Message: Message;

    // No async_trait! Use impl Future for zero allocations
    fn handle(&mut self, msg: Arc<Self::Message>, ctx: &Context<Self>)
        -> impl Future<Output = ()> + Send;

    // Optional lifecycle hooks
    fn on_start(&mut self, ctx: &Context<Self>)
        -> impl Future<Output = ()> + Send {
        async {}
    }

    fn on_error(&mut self, error: Error) -> SupervisionDecision {
        SupervisionDecision::Restart
    }
}

pub struct ActorSystem {
    bus: MessageBus,
    actors: HashMap<ActorId, ActorHandle>,
}

impl ActorSystem {
    pub async fn spawn<A, F>(&mut self, name: &str, factory: F) -> ActorRef<A::Message>
    where
        A: Actor,
        F: FnOnce(&ActorContext) -> A,
    {
        let actor_id = ActorId(rand::random());
        let mailbox_topic = TopicBuilder::actor_mailbox(actor_id);

        // Context provides access to message bus
        let ctx = ActorContext { bus: &self.bus };
        let mut actor = factory(&ctx);

        // Subscribe to actor's mailbox
        let mut mailbox = self.bus.subscriber_for_topic(&mailbox_topic);

        // Spawn supervised task
        let handle = tokio::spawn(async move {
            while let Some(msg) = mailbox.recv().await {
                actor.handle(msg, &ctx).await;
            }
        });

        self.actors.insert(actor_id, ActorHandle { handle, ... });

        ActorRef {
            actor_id,
            publisher: self.bus.publisher_for_topic(&mailbox_topic),
        }
    }
}

pub struct ActorRef<M: Message> {
    actor_id: ActorId,
    publisher: Publisher<M>,
}

impl<M: Message> ActorRef<M> {
    pub async fn send(&self, msg: M) -> Result<()> {
        self.publisher.publish(msg).await
    }
}
```

**Built entirely on Phase 1 primitives!**

---

## Handler Pattern

### Core Principle: Separate Business Logic from Runtime

**Goal**: Make business logic testable, reusable, and runtime-agnostic.

### Handler Structure

```rust
// crates/flash-arbitrage/src/handler.rs

pub struct FlashArbitrageHandler {
    // Dependencies injected via constructor
    signal_publisher: Publisher<ArbitrageSignal>,

    // State
    pool_states: HashMap<Address, PoolState>,
}

impl FlashArbitrageHandler {
    pub fn new(signal_publisher: Publisher<ArbitrageSignal>) -> Self {
        Self {
            signal_publisher,
            pool_states: HashMap::new(),
        }
    }

    // Pure business logic (no runtime knowledge)
    pub async fn handle_swap(&mut self, swap: Arc<SwapEvent>) {
        // Update state
        self.pool_states.entry(swap.pool_address)
            .or_insert_with(PoolState::new)
            .update(&swap);

        // Detect arbitrage
        if let Some(signal) = self.detect_arbitrage(&swap) {
            self.signal_publisher.publish(signal).await.ok();
        }
    }

    fn detect_arbitrage(&self, swap: &SwapEvent) -> Option<ArbitrageSignal> {
        // Pure computation (easily unit tested)
        // ...
    }
}
```

**Benefits**:
- ✅ **Testable**: Inject test publishers, no mocking
- ✅ **Reusable**: Same handler works in pub/sub or actor runtime
- ✅ **Clear boundaries**: Handler doesn't know about supervision, mailboxes, etc.

---

### Testing Handlers

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast;

    #[tokio::test]
    async fn test_arbitrage_detection() {
        // Create test publisher
        let (tx, mut rx) = broadcast::channel(10);
        let test_publisher = Publisher::new(tx);

        // Create handler with test dependencies
        let mut handler = FlashArbitrageHandler::new(test_publisher);

        // Test business logic
        let swap = Arc::new(SwapEvent {
            pool_address: test_pool(),
            amount_in: 1000,
            // ...
        });

        handler.handle_swap(swap).await;

        // Assert on published signals
        let signal = rx.recv().await.unwrap();
        assert!(signal.estimated_profit > 0.0);
    }
}
```

**No runtime, no mocking - just pure logic testing.**

---

## Migration Examples

### Example: Flash Arbitrage Service

#### Today (Pub/Sub)

```rust
// crates/flash-arbitrage/src/main.rs

use mycelium_transport::MessageBus;
use mycelium_protocol::{SwapEvent, ArbitrageSignal};
use flash_arbitrage::FlashArbitrageHandler;

#[tokio::main]
async fn main() {
    // Load config and create message bus
    let topology = mycelium_config::load("config.toml").unwrap();
    let bus = MessageBus::from_topology(topology, "flash-arbitrage");

    // Set up pub/sub
    let mut swaps = bus.subscriber::<SwapEvent>();
    let signals = bus.publisher::<ArbitrageSignal>();

    // Create handler (dependency injection)
    let mut handler = FlashArbitrageHandler::new(signals);

    // Simple message loop
    println!("Flash Arbitrage Service Running...");
    while let Some(swap) = swaps.recv().await {
        handler.handle_swap(swap).await;
    }
}
```

**Lines of code**: 15
**Complexity**: Low
**Supervision**: External (systemd/k8s)

---

#### Later (With Actors)

**Step 1**: Add Actor trait to handler

```rust
// crates/flash-arbitrage/src/handler.rs (add this impl)

use mycelium_actors::{Actor, Context};

impl Actor for FlashArbitrageHandler {
    type Message = SwapEvent;

    fn handle(&mut self, msg: Arc<SwapEvent>, _ctx: &Context<Self>)
        -> impl Future<Output = ()> + Send
    {
        // Delegate to existing method (no refactoring!)
        self.handle_swap(msg)
    }
}
```

**Step 2**: Update main.rs to use ActorSystem

```rust
// crates/flash-arbitrage/src/main.rs (NEW)

use mycelium_actors::ActorSystem;
use flash_arbitrage::FlashArbitrageHandler;

#[tokio::main]
async fn main() {
    // Actor system handles config and message bus
    let mut system = ActorSystem::from_config("config.toml").await.unwrap();

    // Spawn supervised actor
    system.spawn("flash-arbitrage", |ctx| {
        let signals = ctx.publisher::<ArbitrageSignal>();
        FlashArbitrageHandler::new(signals)
    })
    .with_restart_policy(RestartPolicy::Always)
    .await;

    // System supervises all actors
    println!("Flash Arbitrage Actor Running...");
    system.run().await;
}
```

**Lines of code**: 12
**Complexity**: Low
**Supervision**: Built-in (actor-level)

**Migration cost**: 5 lines added to handler, main.rs simplified!

---

### Example: Polygon Adapter with Multi-Provider Redundancy

#### Today (Pub/Sub + Manual Supervision)

```rust
// Spawn 3 WebSocket tasks manually
for provider in ["wss://p1.com", "wss://p2.com", "wss://p3.com"] {
    let publisher = bus.publisher::<PoolStateUpdate>();

    tokio::spawn(async move {
        loop {
            match connect_websocket(provider).await {
                Ok(ws) => process_events(ws, publisher.clone()).await,
                Err(e) => {
                    tracing::error!("Provider {} failed: {}", provider, e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    });
}
```

**Issues**:
- ❌ If one task panics, no automatic restart
- ❌ Manual error handling and retry logic
- ❌ No supervision tree

---

#### Later (With Actors)

```rust
// Each provider is a supervised actor
struct ProviderActor {
    url: String,
    publisher: Publisher<PoolStateUpdate>,
}

impl Actor for ProviderActor {
    type Message = ControlMessage; // Start/Stop/Pause

    async fn on_start(&mut self, _ctx: &Context<Self>) {
        loop {
            let ws = connect_websocket(&self.url).await?;
            while let Some(event) = ws.next().await {
                self.publisher.publish(parse_event(event)).await;
            }
            // Crashes propagate to supervisor
        }
    }

    async fn on_error(&mut self, e: Error) -> SupervisionDecision {
        tracing::error!("Provider {} failed: {}", self.url, e);
        SupervisionDecision::Restart // Supervisor handles retry
    }
}

// Main
let mut system = ActorSystem::new();

for url in ["wss://p1.com", "wss://p2.com", "wss://p3.com"] {
    system.spawn(url, |ctx| {
        ProviderActor {
            url: url.to_string(),
            publisher: ctx.publisher::<PoolStateUpdate>(),
        }
    })
    .with_restart_policy(RestartPolicy::Always)
    .with_backoff(ExponentialBackoff::default())
    .await;
}

system.run().await;
```

**Benefits**:
- ✅ Automatic restart on panic (10-50ms recovery)
- ✅ Exponential backoff handled by supervisor
- ✅ Other providers unaffected by failures
- ✅ Cleaner error handling

---

## Implementation Details

### Sequence Diagram: Pub/Sub Message Flow

```
Publisher                  MessageBus              Subscriber A    Subscriber B
   │                           │                         │              │
   │ publish(msg)              │                         │              │
   ├──────────────────────────>│                         │              │
   │                           │ Arc::clone(envelope)    │              │
   │                           ├────────────────────────>│              │
   │                           │                         │              │
   │                           │ Arc::clone(envelope)    │              │
   │                           ├─────────────────────────┼─────────────>│
   │                           │                         │              │
   │                           │      recv().await       │              │
   │                           │<────────────────────────┤              │
   │                           │                         │              │
   │                           │   Arc<Message>          │              │
   │                           ├────────────────────────>│              │
```

**Key**: Single message, multiple Arc clones (zero-copy sharing).

---

### Sequence Diagram: Actor Message Flow

```
ActorRef               MessageBus              Actor Mailbox       Actor
   │                       │                         │               │
   │ send(msg)             │                         │               │
   ├──────────────────────>│                         │               │
   │                       │ publish to             │               │
   │                       │ "actor.123abc"          │               │
   │                       ├────────────────────────>│               │
   │                       │                         │               │
   │                       │      recv().await       │               │
   │                       │<────────────────────────┤               │
   │                       │                         │               │
   │                       │   Arc<Message>          │               │
   │                       ├─────────────────────────┼──────────────>│
   │                       │                         │               │
   │                       │                         │  handle(msg)  │
   │                       │                         │<──────────────┤
```

**Key**: Actor mailbox is just a single-subscriber topic.

---

### Memory Layout: Envelope

```rust
pub struct Envelope {
    type_id: u16,              // 2 bytes
    _pad1: [u8; 6],            // 6 bytes padding (alignment)
    topic: String,             // 24 bytes (ptr + len + cap)
    destination: Option<Destination>, // 16 bytes (Option<enum>)
    correlation_id: Option<CorrelationId>, // 24 bytes (Option<u128>)
    payload: Arc<dyn Any>,     // 16 bytes (fat pointer)
}
// Total: 88 bytes per envelope
```

**Optimization**: If metadata is `None`, it's just a discriminant (1 byte + padding). Compiler may optimize.

---

## Decision Tree

### Should I Use Actors?

```
Start: Building a new service

┌─ Does the service have multiple independent failure domains?
│  (e.g., 3 WebSocket connections, each can fail separately)
│
├─ YES → Consider actors for fault isolation
│  └─ Can you tolerate 5-10s recovery on failure?
│     ├─ YES → Pub/sub is fine (use k8s restart)
│     └─ NO → Use actors (10-50ms recovery)
│
└─ NO → Pub/sub is simpler
   └─ Does the service need per-entity state with complex lifecycle?
      ├─ YES → Actors may help
      └─ NO → Pub/sub is correct
```

### Example Decisions Across Use Cases

**DeFi Arbitrage** (pub/sub-heavy):
```
Polygon Adapter:
  - Multiple WebSocket connections ✅
  - Need fast recovery (<100ms) ✅
  → Use actors

Pool State Manager:
  - Centralized global state ✅
  - Global liquidity graph ✅
  → Use pub/sub (exception to actor pattern)

Arbitrage Detector:
  - Stateless computation ✅
  → Use pub/sub
```

**CEX Trading** (actor-heavy):
```
Each Exchange Adapter:
  - Independent connection ✅
  - Isolated failure domain ✅
  - Per-exchange rate limits ✅
  → Use actors (20+ exchange actors)

Price Aggregator:
  - Receives from all exchanges ✅
  - Broadcasts to strategies ✅
  → Use pub/sub

Each Strategy:
  - Isolated state/positions ✅
  - Independent risk limits ✅
  → Use actors (100+ strategy actors)
```

**Market Making** (actor-heavy):
```
Per-Venue Market Maker:
  - Venue-specific order book ✅
  - Independent spread logic ✅
  → Use actors (one per venue)

Inventory Manager:
  - Aggregates positions ✅
  - Broadcasts rebalancing signals ✅
  → Use pub/sub
```

**Verdict**: Framework needs both pub/sub (broadcast) and actors (isolation). Most services use actors as complexity scales.

---

## Summary

### Phase 1: Actor-Ready Foundation (This Week)

1. **Fix existing tests** ✅
2. **Add routing types** (Destination, ActorId, CorrelationId) ✅
3. **Enhance Envelope** (optional metadata) ✅
4. **Expose dynamic topic API** ✅
5. **Add TopicBuilder helpers** ✅
6. **Document handler pattern** ✅

**Total effort**: ~1-2 days
**Code changes**: ~100 lines
**Breaking changes**: 0
**Enables**: Full actor system in Phase 2

---

### Phase 2: Full Actor System (Next 1-2 Weeks) **← RECOMMENDED**

7. **Create mycelium-actors crate**
   - Actor trait (zero-allocation with impl Future)
   - ActorSystem (spawn, lifecycle management)
   - Supervision (restart policies, backoff)
   - Hierarchical supervision trees

8. **Build proof-of-concept examples**
   - CEX adapter actor (multi-connection supervision)
   - Strategy actor (isolated state/PnL)
   - Multi-strategy example (demonstrate scalability)

**Total effort**: ~1 week
**New crate**: mycelium-actors (~500-800 lines)
**Existing services**: Work unchanged
**Value**: Enables CEX trading, market making, multi-strategy, TradFi

---

### Phase 3: Production Services (Ongoing)

9. **DeFi arbitrage** (uses pub/sub - special case)
10. **CEX trading** (uses actors)
11. **Market making** (uses actors)
12. **Multi-strategy portfolio** (uses actors)
13. **Risk management** (uses actors)

**Mycelium becomes a complete trading framework supporting diverse use cases**

---

### Guiding Principles

1. **Build both patterns** - Pub/sub and actors serve different use cases
2. **Handler pattern** - Separate business logic from runtime
3. **Actors reduce complexity** - At scale, actors lower code growth vs manual management
4. **Progressive enhancement** - Migration path is clear and low-cost
5. **Framework completeness** - Support DeFi, CEX, market making, TradFi from day one

---

## Open Questions

1. **Serialization**: **Decision: Stick with `zerocopy` using `FixedVec`.**
   - The custom `FixedVec` implementation provides the performance of `zerocopy` while allowing for bounded, variable-length data, which is ideal for our domain.
   - This approach avoids dynamic heap allocations on the hot path and gives us direct control over memory layouts.
   - It is preferred over `rkyv` as it avoids an additional major dependency and fits our specific performance needs.

2. **Supervision scope**: Should mycelium-actors include hierarchical supervision (Erlang-style)?
   - **Recommendation**: Start with minimal (restart policies, backoff), add hierarchical if needed
   - Erlang-style supervision (one-for-one, one-for-all, rest-for-one) adds complexity
   - CEX/strategy use cases likely only need simple restart policies
   - Can add hierarchical later if multi-exchange coordination requires it

3. **Request/reply pattern**: Build now or wait?
   - **Recommendation**: Wait for first service that needs it
   - Correlation IDs are in place for when we need it
   - Most trading patterns are event-driven, not request/reply
   - Add when we have concrete use case (e.g., order status queries)

---

## References

- [Akka Actor Model](https://doc.akka.io/docs/akka/current/typed/guide/actors-intro.html)
- [Actix Actor Framework](https://actix.rs/)
- [Erlang/OTP Supervision Trees](https://www.erlang.org/doc/design_principles/sup_princ.html)
- [Mycelium Transport Layer](../README.md)

---

**Last Updated**: 2025-01-03
**Status**: Design Review → Implementation
**Reviewers**: @daws, @gemini
