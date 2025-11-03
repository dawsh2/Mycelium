# Actor System Evolution Path

**Purpose**: Document how Mycelium can grow from pure pub/sub to full actor system without breaking changes.

**Status**: Phase 1 complete. Phase 2 deferred until concrete use case emerges.

---

## Current State (Phase 1 Complete)

### What's Implemented

**Foundation for future actor system** - all pieces in place:

1. **Routing types** (`mycelium-protocol/src/routing.rs`):
   - `ActorId` - unique actor identifiers
   - `CorrelationId` - request/reply correlation
   - `Destination` - routing hints (Broadcast/Unicast/Partition)

2. **Enhanced Envelope** with optional metadata:
   - `destination: Option<Destination>` - currently unused, always None
   - `correlation_id: Option<CorrelationId>` - for future request/reply

3. **Dynamic topic API** (`mycelium-transport/src/bus.rs`):
   - `publisher_for_topic<M>(&self, topic: &str)` - explicit topic control
   - `subscriber_for_topic<M>(&self, topic: &str)` - explicit subscriptions
   - Existing `publisher<M>()` and `subscriber<M>()` unchanged

4. **TopicBuilder helpers** (`mycelium-transport/src/topics.rs`):
   - `actor_mailbox(actor_id)` → `"actor.0000000000123abc"`
   - `partition(base, n)` → `"orderbook.0"`
   - `broadcast(name)` → `"name"`

5. **ManagedService trait** with lifecycle hooks:
   - `initialize()`, `shutdown()`, `health()`, `on_error()`
   - Actor-like lifecycle pattern already exists

### Key Architectural Decisions

**Decision 1: Mailbox = Topic**
- Actor mailboxes are pub/sub topics with single subscriber
- Actor 123's mailbox → topic "actor.0000000000123abc"
- Simple, efficient, leverages existing transport layer

**Decision 2: Message trait removed Copy requirement**
- Allows future messages with `Vec<T>`, `String`, heap data
- Currently use `FixedVec` to maintain zerocopy performance
- Ready for evolution to non-Copy messages when needed

**Decision 3: Handler Pattern**
- Business logic separated from runtime (pub/sub vs actors)
- Handlers inject dependencies via constructor
- Same handler works in both pub/sub and actor contexts

---

## When to Implement Phase 2

### Trigger Scenarios

**Build Phase 2 actor system when you hit ANY of these:**

1. **Multi-exchange CEX trading**
   - Need 10+ exchange connections with isolated failure domains
   - Binance crash shouldn't affect Coinbase
   - Current: Manual tokio::spawn + manual supervision
   - With actors: `system.spawn()` with automatic supervision

2. **Multi-strategy portfolio**
   - Need 20+ strategies with per-strategy risk limits and PnL tracking
   - Strategy 1 crash shouldn't kill other 99 strategies
   - Current: Not feasible without actors
   - With actors: One actor per strategy, isolated state

3. **WebSocket supervision requirements**
   - Need <100ms recovery time on connection failures
   - Current: Process-level restart (5-10s downtime)
   - With actors: Actor-level restart (10-50ms)

4. **Code complexity explosion**
   - Manual supervision code growing linearly with services
   - Need hierarchical supervision patterns
   - Current: Custom supervision per service
   - With actors: Standardized supervision policies

### Don't Build If

- Using only DeFi arbitrage (global liquidity graph doesn't need actors)
- Comfortable with process-level supervision (systemd/k8s restart)
- Managing <10 stateful services (manual management still feasible)

---

## Migration Path: Pub/Sub → Actors

### Example: Flash Arbitrage Service

**Today (Pure Pub/Sub)**:
```rust
// crates/flash-arbitrage/src/handler.rs
pub struct FlashArbitrageHandler {
    signal_publisher: Publisher<ArbitrageSignal>,
    pool_states: HashMap<Address, PoolState>,
}

impl FlashArbitrageHandler {
    pub async fn handle_swap(&mut self, swap: Arc<SwapEvent>) {
        self.pool_states.entry(swap.pool_address)
            .or_insert_with(PoolState::new)
            .update(&swap);

        if let Some(signal) = self.detect_arbitrage(&swap) {
            self.signal_publisher.publish(signal).await.ok();
        }
    }
}

// crates/flash-arbitrage/src/main.rs
#[tokio::main]
async fn main() {
    let bus = MessageBus::new();
    let mut swaps = bus.subscriber::<SwapEvent>();
    let signals = bus.publisher::<ArbitrageSignal>();

    let mut handler = FlashArbitrageHandler::new(signals);

    while let Some(swap) = swaps.recv().await {
        handler.handle_swap(swap).await;
    }
}
```

**Step 1: Add Actor trait (2 minutes)**:
```rust
// crates/flash-arbitrage/src/handler.rs
use mycelium_actors::{Actor, Context};

impl Actor for FlashArbitrageHandler {
    type Message = SwapEvent;

    fn handle(&mut self, msg: Arc<SwapEvent>, _ctx: &Context<Self>)
        -> impl Future<Output = ()> + Send
    {
        // Delegate to existing method - zero refactoring!
        self.handle_swap(msg)
    }
}
```

**Step 2: Update main.rs (5 minutes)**:
```rust
// crates/flash-arbitrage/src/main.rs
use mycelium_actors::ActorSystem;

#[tokio::main]
async fn main() {
    let mut system = ActorSystem::from_config("config.toml").await.unwrap();

    system.spawn("flash-arbitrage", |ctx| {
        let signals = ctx.publisher::<ArbitrageSignal>();
        FlashArbitrageHandler::new(signals)
    })
    .with_restart_policy(RestartPolicy::Always)
    .await;

    system.run().await;
}
```

**Migration cost**: 7 lines of code, business logic unchanged.

---

## Phase 2 Implementation Plan

### New Crate: mycelium-actors

**Core API** (~500-800 lines):

```rust
// Actor trait
pub trait Actor {
    type Message: Message;

    fn handle(&mut self, msg: Arc<Self::Message>, ctx: &Context<Self>)
        -> impl Future<Output = ()> + Send;

    fn on_start(&mut self, ctx: &Context<Self>)
        -> impl Future<Output = ()> + Send { async {} }

    fn on_error(&mut self, error: Error) -> SupervisionDecision {
        SupervisionDecision::Restart
    }
}

// Actor system
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
        let actor_id = ActorId::new();
        let mailbox_topic = TopicBuilder::actor_mailbox(actor_id);

        // Create actor with context
        let ctx = ActorContext { bus: &self.bus };
        let mut actor = factory(&ctx);

        // Subscribe to mailbox (single subscriber)
        let mut mailbox = self.bus.subscriber_for_topic(&mailbox_topic);

        // Spawn supervised task
        let handle = tokio::spawn(async move {
            while let Some(msg) = mailbox.recv().await {
                actor.handle(msg, &ctx).await;
            }
        });

        self.actors.insert(actor_id, ActorHandle { handle, /* ... */ });

        ActorRef {
            actor_id,
            publisher: self.bus.publisher_for_topic(&mailbox_topic),
        }
    }
}

// Sending messages to actors
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

### Supervision Policies

**Start simple, grow as needed**:

```rust
pub enum RestartPolicy {
    Never,               // Don't restart on failure
    Always,              // Restart immediately
    OnFailure,           // Restart on error, not on clean exit
    WithBackoff(BackoffStrategy),  // Exponential backoff
}

pub enum SupervisionDecision {
    Restart,             // Restart this actor
    Stop,                // Stop permanently
    Escalate,            // Let parent supervisor decide
}
```

**Future enhancements** (only if needed):
- Hierarchical supervision (one-for-one, one-for-all, rest-for-one)
- Supervision trees
- Circuit breakers

---

## What Stays Compatible

### No Breaking Changes

1. **Existing pub/sub code** - works unchanged
2. **Message definitions** - no modifications needed
3. **Transport layer** - actors use same transport
4. **Configuration** - topology config unchanged
5. **Handler pattern** - business logic reusable

### New APIs (Additive Only)

1. **mycelium-actors crate** - new dependency, opt-in
2. **Actor trait** - implement on existing handlers
3. **ActorSystem** - new runtime, parallel to MessageBus
4. **ActorRef** - typed handle to actors

### Coexistence

**Pub/sub and actors work together**:

```rust
// Pub/sub for broadcast
let pool_updates = bus.subscriber::<PoolStateUpdate>();

// Actors for stateful services
let strategy_actor = system.spawn("my-strategy", |ctx| {
    StrategyActor::new(ctx.publisher::<TradeSignal>())
}).await;

// Hybrid: actors publish to pub/sub
// Actors subscribe to pub/sub
// Same message bus underneath
```

---

## Technical Deep Dive

### How Mailboxes Work

**Actor mailbox = dedicated topic with single subscriber**:

```
Broadcast pub/sub:
Topic: "pool_updates" → [Sub A, Sub B, Sub C]

Actor mailboxes:
Topic: "actor.0000000000123abc" → [Actor 123 only]
Topic: "actor.0000000000456def" → [Actor 456 only]
```

**Benefits**:
- True isolation (actor 123's messages never touch actor 456)
- Per-actor backpressure (slow actor doesn't affect others)
- Simple (no filtering logic)
- Efficient (no wasted work)

**Trade-off**: More topics, but HashMap lookup is O(1).

### Memory Overhead

**Per actor**:
- ActorId: 8 bytes
- Mailbox subscriber: ~80 bytes (tokio channel + state)
- ActorHandle: ~100 bytes (task handle + metadata)
- Total: ~200 bytes per actor

**Scalability**:
- 1,000 actors: 200 KB
- 10,000 actors: 2 MB
- 100,000 actors: 20 MB (probably excessive for trading use cases)

### Performance Characteristics

**Local transport (same process)**:
- Actor message send: ~200ns (Arc clone + channel send)
- Actor supervision restart: 10-50ms
- Process supervision restart: 5-10s
- **100-1000x faster recovery**

---

## Decision Tree

### Should I Use Actors for This Service?

```
┌─ Does service have multiple independent failure domains?
│  (e.g., 3 WebSocket connections, each can fail separately)
│
├─ YES → Consider actors for fault isolation
│  └─ Can you tolerate 5-10s recovery on failure?
│     ├─ YES → Pub/sub is fine (use k8s restart)
│     └─ NO → Use actors (10-50ms recovery)
│
└─ NO → Pub/sub is simpler
   └─ Does service need per-entity state with complex lifecycle?
      ├─ YES → Actors may help
      └─ NO → Pub/sub is correct
```

### Use Case Examples

**DeFi Arbitrage**:
- Polygon Adapter: Multiple WebSockets → **Use actors**
- Pool State Manager: Global liquidity graph → **Use pub/sub** (exception)
- Arbitrage Detector: Stateless computation → **Use pub/sub**

**CEX Trading** (future):
- Each Exchange Adapter: Independent connections → **Use actors** (20+ actors)
- Price Aggregator: Broadcast to strategies → **Use pub/sub**
- Each Strategy: Isolated state/PnL → **Use actors** (100+ actors)

**Market Making** (future):
- Per-Venue Market Maker: Venue-specific state → **Use actors**
- Inventory Manager: Aggregate positions → **Use pub/sub**

---

## Timeline and Effort

### Phase 2 Implementation (When Needed)

**Estimated effort**: 3-5 days

**Breakdown**:
1. Core actor traits: 4 hours
2. ActorSystem spawn/lifecycle: 8 hours
3. Supervision policies: 8 hours
4. Integration with MessageBus: 4 hours
5. Testing and examples: 8 hours

**Deliverables**:
- mycelium-actors crate (~800 lines)
- 3 examples (CEX adapter, strategy actor, multi-exchange)
- Documentation updates

### Migration Path (Per Service)

**Estimated effort per service**: 30 minutes - 2 hours

**Depends on**:
- Service complexity
- Whether handler pattern already used
- Supervision requirements

---

## Summary

### Current Architecture Preserves Evolution

**Key decisions that enable actors without breaking changes**:

1. Message trait removed Copy requirement
2. Routing types (ActorId, Destination) in place
3. Dynamic topic API exposed
4. Handler pattern established
5. Envelope has optional metadata fields

### When to Build Phase 2

**Wait for concrete use case**:
- CEX trading (multiple exchanges)
- Multi-strategy portfolio (20+ strategies)
- Complex WebSocket supervision (<100ms recovery)

**Don't build prematurely**:
- DeFi arbitrage alone doesn't need actors
- YAGNI principle - build when you hit pain points

### Zero Risk of Wrong Abstractions

**Current foundation is minimal and flexible**:
- No over-engineered supervision (can add later)
- No premature request/reply (CorrelationId ready when needed)
- No actor-specific wire protocol (reuses pub/sub)

**Migration path is proven**:
- Akka, Actix, Orleans all use "mailbox = queue" pattern
- Our "mailbox = topic" is simpler and faster

---

**Last Updated**: 2025-01-03
**Next Review**: When implementing first non-DeFi use case (CEX trading, market making, or multi-strategy)
