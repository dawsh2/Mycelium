# Mycelium Actor System Design

## Philosophy

**Core Principle**: Same actor code, different performance characteristics based on deployment topology.

```rust
// Developer writes this once:
actor_ref.send(message).await?;

// Runtime chooses transport:
// - Monolith:      Arc<T> clone         (~200ns, zero-copy)
// - Multi-process: Unix domain socket   (~50μs, zero-copy via shared mem)
// - Distributed:   TCP with rkyv        (~500μs, zero-copy deserialization)
```

**Design Goals**:
1. **Adaptive Transport** - Location transparency with optimal performance
2. **Erlang-style Supervision** - Fault isolation and recovery
3. **Type Safety** - Compile-time message type checking
4. **Zero-Copy** - Minimal allocations in hot path
5. **Backpressure** - Bounded mailboxes with flow control
6. **Observable** - Built-in tracing and metrics

---

## Core Abstractions

### 1. The Actor Trait

```rust
/// Core actor behavior
#[async_trait]
pub trait Actor: Send + 'static {
    /// The message type this actor handles
    type Message: Message;
    
    /// Mutable state (separate from actor logic for initialization safety)
    type State: Send + 'static;
    
    /// Arguments passed during spawn
    type Arguments: Send + 'static;
    
    /// Called once before the actor starts processing messages
    async fn pre_start(
        &self,
        ctx: &ActorContext<Self::Message>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorError>;
    
    /// Handle a message
    async fn handle(
        &self,
        ctx: &ActorContext<Self::Message>,
        message: Self::Message,
        state: &mut Self::State,
    ) -> Result<(), ActorError>;
    
    /// Called when the actor is stopping (graceful shutdown)
    async fn post_stop(&self, state: Self::State) {
        // Optional cleanup
    }
}
```

**Design Notes**:
- Separates actor logic (`&self`) from mutable state (`&mut State`)
- Prevents initialization issues (state created in `pre_start()`)
- Erlang pattern: `gen_server:init/1` → `pre_start()`
- Async trait for network I/O, database calls, etc.

---

### 2. The Message Trait

```rust
/// A message that can be sent between actors
pub trait Message: Send + Sync + 'static {
    /// Whether this message can be shared via Arc<T> (default: true)
    fn arc_shareable() -> bool {
        true
    }
    
    /// Whether this message can be serialized for remote transport
    fn serializable() -> bool {
        false
    }
}

// Auto-implement for most types
impl<T: Send + Sync + 'static> Message for T {}
```

**Key Requirements**:
- `Send + Sync` - Required for `Arc<T>` sharing across threads
- `'static` - No lifetime parameters (actors are long-lived)

**Why Sync?**
```rust
// Arc<T> requires T: Send + Sync
let msg = Arc::new(MyMessage { ... });
actor_ref.send(msg).await;  // Shares across threads
```

---

### 3. ActorRef - The Communication Handle

```rust
/// Typed reference to an actor
pub struct ActorRef<M: Message> {
    inner: ActorCell,
    _phantom: PhantomData<M>,
}

impl<M: Message> ActorRef<M> {
    /// Send a message (async, waits for mailbox space if full)
    pub async fn send(&self, message: M) -> Result<(), SendError>;
    
    /// Try to send (non-blocking, fails if mailbox full)
    pub fn try_send(&self, message: M) -> Result<(), TrySendError>;
    
    /// Send and wait for reply (RPC-style)
    pub async fn call<R>(&self, f: impl FnOnce(RpcReplyPort<R>) -> M) -> Result<R, CallError>
    where
        R: Send + 'static;
    
    /// Get the actor's ID
    pub fn id(&self) -> ActorId;
    
    /// Check if the actor is still alive
    pub fn is_alive(&self) -> bool;
    
    /// Stop the actor gracefully
    pub async fn stop(&self) -> Result<(), StopError>;
}
```

**Clone is cheap**: `ActorRef` is `Arc<ActorCell>` internally.

---

### 4. ActorCell - Internal Representation

```rust
/// Internal actor cell (not typically used by developers)
pub struct ActorCell {
    id: ActorId,
    name: Option<ActorName>,
    mailbox: Mailbox,
    supervisor: Option<Weak<ActorCell>>,
    monitors: Vec<Weak<ActorCell>>,
}
```

**ActorId variants**:
```rust
pub enum ActorId {
    /// Actor in the same process (Arc transport)
    Local { pid: u64 },
    
    /// Actor on the same machine (Unix socket transport)
    Machine { node_id: NodeId, pid: u64 },
    
    /// Actor on a different machine (TCP transport)
    Remote { node_id: NodeId, pid: u64 },
}
```

---

### 5. The Mailbox

```rust
/// Actor's message queue with prioritization
pub struct Mailbox {
    /// System messages (highest priority)
    system_rx: mpsc::Receiver<SystemMessage>,
    
    /// User messages (normal priority)
    user_rx: mpsc::Receiver<BoxedMessage>,
    
    /// Mailbox capacity (for backpressure)
    capacity: usize,
}

pub enum SystemMessage {
    Stop,                         // Graceful shutdown
    Terminate,                    // Immediate shutdown
    SupervisionEvent(SupervisionEvent),
}
```

**Message Processing Order**:
1. System messages (can interrupt work)
2. User messages (FIFO)

**Backpressure**:
- Mailbox bounded at `capacity` (default: 1000)
- `send()` blocks if full (async backpressure)
- `try_send()` returns error if full

---

### 6. ActorContext

```rust
/// Context provided to actor during message handling
pub struct ActorContext<M: Message> {
    /// Reference to self
    myself: ActorRef<M>,
    
    /// Actor registry (for discovery)
    registry: Arc<ActorRegistry>,
    
    /// Supervisor (if any)
    supervisor: Option<ActorRef<SupervisionMessage>>,
    
    /// Spawn child actors
    pub async fn spawn<A: Actor>(
        &self,
        actor: A,
        args: A::Arguments,
    ) -> Result<ActorRef<A::Message>, SpawnError>;
    
    /// Find an actor by name
    pub fn find<M2: Message>(&self, name: &str) -> Option<ActorRef<M2>>;
    
    /// Stop self
    pub async fn stop(&self);
}
```

**Usage Example**:
```rust
async fn handle(&self, ctx: &ActorContext<Self::Message>, msg: MyMessage, state: &mut State) {
    // Spawn a child
    let child = ctx.spawn(ChildActor, ChildArgs { ... }).await?;
    
    // Send to child
    child.send(ChildMessage::Process).await?;
    
    // Find another actor
    if let Some(other) = ctx.find::<OtherMessage>("data-processor") {
        other.send(OtherMessage::Update).await?;
    }
}
```

---

## Adaptive Transport Layer

### Transport Selection Logic

```rust
impl ActorRef<M> {
    pub async fn send(&self, message: M) -> Result<(), SendError> {
        match self.inner.id {
            ActorId::Local { .. } => {
                // Same process - use Arc<T>
                let arc_msg = Arc::new(message);
                self.inner.mailbox.send_arc(arc_msg).await
            }
            ActorId::Machine { node_id, pid } => {
                // Same machine - use Unix socket (zero-copy via shared mem)
                let serialized = rkyv::to_bytes(&message)?;
                self.unix_transport.send(node_id, pid, serialized).await
            }
            ActorId::Remote { node_id, pid } => {
                // Different machine - use TCP with rkyv
                let serialized = rkyv::to_bytes(&message)?;
                self.tcp_transport.send(node_id, pid, serialized).await
            }
        }
    }
}
```

### Transport Implementations

#### Arc Transport (Same Process)

```rust
pub struct ArcTransport {
    // Just forwards Arc<M> to local mailbox
}

impl ArcTransport {
    async fn send<M: Message>(&self, mailbox: &Mailbox, msg: Arc<M>) {
        // Zero-copy: just clones the Arc (atomic refcount increment)
        mailbox.user_tx.send(BoxedMessage::Arc(msg)).await
    }
}
```

**Performance**: ~200ns (Arc clone + channel send)

---

#### Unix Socket Transport (Same Machine)

```rust
pub struct UnixTransport {
    sockets: HashMap<NodeId, UnixStream>,
}

impl UnixTransport {
    async fn send(&self, node_id: NodeId, pid: u64, bytes: Vec<u8>) {
        let socket = self.sockets.get(&node_id)?;
        
        // Send via Unix domain socket
        // OS optimizes via shared memory (zero-copy on Linux)
        socket.send(&bytes).await?;
    }
}
```

**Performance**: ~50μs (syscall overhead, but zero-copy)

**Why Unix sockets?**
- Faster than TCP loopback (no network stack)
- Zero-copy on Linux (shared memory pages)
- File permissions for security

---

#### TCP Transport (Different Machines)

```rust
pub struct TcpTransport {
    connections: HashMap<NodeId, TcpStream>,
}

impl TcpTransport {
    async fn send(&self, node_id: NodeId, pid: u64, bytes: Vec<u8>) {
        let conn = self.connections.get(&node_id)?;
        
        // Send via TCP
        conn.write_all(&bytes).await?;
    }
}
```

**Performance**: ~500μs (network latency)

**Serialization**: `rkyv` for zero-copy deserialization
- 2-4x faster than bincode
- Validates + reads without allocation

---

### BoxedMessage - Type-Erased Transport

```rust
pub enum BoxedMessage {
    /// Arc-wrapped message (same process)
    Arc(Arc<dyn Any + Send + Sync>),
    
    /// Serialized message (remote)
    Bytes {
        type_id: u64,           // For deserialization
        payload: Vec<u8>,       // rkyv bytes
    },
}

impl BoxedMessage {
    /// Downcast to concrete type
    pub fn downcast<M: Message>(self) -> Result<M, DowncastError> {
        match self {
            BoxedMessage::Arc(arc) => {
                // Downcast Arc
                let arc_any = arc.downcast::<M>()
                    .map_err(|_| DowncastError::TypeMismatch)?;
                
                // Try to unwrap (if we're the only owner)
                match Arc::try_unwrap(arc_any) {
                    Ok(msg) => Ok(msg),
                    Err(arc) => Ok((*arc).clone()),  // Clone if shared
                }
            }
            BoxedMessage::Bytes { type_id, payload } => {
                // Deserialize with rkyv
                rkyv::from_bytes::<M>(&payload)
                    .map_err(|_| DowncastError::DeserializeFailed)
            }
        }
    }
}
```

---

## Supervision Trees

### Erlang-Style Supervision

```rust
pub enum SupervisionStrategy {
    /// Restart failed child (most common)
    RestartChild {
        max_restarts: u32,
        within: Duration,
    },
    
    /// Restart all children (for dependent children)
    RestartAll,
    
    /// Stop supervisor (escalate to parent)
    Escalate,
}

pub struct SupervisorConfig {
    strategy: SupervisionStrategy,
    children: Vec<ChildSpec>,
}

pub struct ChildSpec {
    name: String,
    actor: Box<dyn Actor>,
    args: Box<dyn Any + Send>,
    restart: RestartPolicy,
}

pub enum RestartPolicy {
    Permanent,   // Always restart
    Transient,   // Restart only on error
    Temporary,   // Never restart
}
```

### Supervisor Actor

```rust
pub struct Supervisor {
    config: SupervisorConfig,
}

#[async_trait]
impl Actor for Supervisor {
    type Message = SupervisionMessage;
    type State = SupervisorState;
    type Arguments = SupervisorConfig;
    
    async fn handle(
        &self,
        ctx: &ActorContext<Self::Message>,
        message: Self::Message,
        state: &mut Self::State,
    ) -> Result<(), ActorError> {
        match message {
            SupervisionMessage::ChildFailed { child_id, error } => {
                match self.config.strategy {
                    SupervisionStrategy::RestartChild { max_restarts, within } => {
                        if state.should_restart(child_id, max_restarts, within) {
                            self.restart_child(ctx, child_id).await?;
                        } else {
                            // Too many restarts - escalate
                            ctx.supervisor.notify(SupervisionEvent::Escalate)?;
                        }
                    }
                    SupervisionStrategy::RestartAll => {
                        self.restart_all_children(ctx).await?;
                    }
                    SupervisionStrategy::Escalate => {
                        ctx.supervisor.notify(SupervisionEvent::Escalate)?;
                    }
                }
            }
        }
        Ok(())
    }
}
```

### Example Supervision Tree

```
                    ApplicationSupervisor
                            |
            +---------------+---------------+
            |                               |
    DataIngestorSupervisor          StrategySupervisor
            |                               |
    +-------+-------+               +-------+-------+
    |               |               |               |
PolygonAdapter  EthAdapter    FlashArb        MktMaker
```

**Crash behavior**:
1. `PolygonAdapter` crashes
2. `DataIngestorSupervisor` receives `ChildFailed` event
3. Restarts `PolygonAdapter` (up to max_restarts)
4. If too many restarts → escalates to `ApplicationSupervisor`
5. `ApplicationSupervisor` decides: restart subtree or shutdown

---

## Actor Lifecycle

```
[Created] 
   ↓
[pre_start()] 
   ↓
[Running] ←→ [Handling Messages]
   ↓
[Stopping]
   ↓
[post_stop()]
   ↓
[Terminated]
```

### Lifecycle Hooks

```rust
#[async_trait]
impl Actor for MyActor {
    // 1. Called once before processing messages
    async fn pre_start(&self, ctx: &ActorContext<Self::Message>, args: Args) 
        -> Result<State, ActorError> 
    {
        // Initialize resources
        let db = Database::connect(args.db_url).await?;
        Ok(State { db })
    }
    
    // 2. Called for each message
    async fn handle(&self, ctx: &ActorContext<Self::Message>, msg: Msg, state: &mut State) 
        -> Result<(), ActorError> 
    {
        // Process message
        state.db.insert(msg.data).await?;
        Ok(())
    }
    
    // 3. Called when stopping (graceful shutdown)
    async fn post_stop(&self, state: State) {
        // Cleanup resources
        state.db.close().await;
    }
}
```

---

## Spawning Actors

### Basic Spawn

```rust
use mycelium_core::{Actor, ActorRef};

// Define actor
struct Counter;

#[async_trait]
impl Actor for Counter {
    type Message = CounterMessage;
    type State = u64;
    type Arguments = u64;  // Initial count
    
    async fn pre_start(&self, _ctx: &ActorContext<Self::Message>, initial: u64) 
        -> Result<u64, ActorError> 
    {
        Ok(initial)
    }
    
    async fn handle(&self, _ctx: &ActorContext<Self::Message>, msg: CounterMessage, count: &mut u64) 
        -> Result<(), ActorError> 
    {
        match msg {
            CounterMessage::Increment => *count += 1,
            CounterMessage::Get(reply) => reply.send(*count),
        }
        Ok(())
    }
}

// Spawn it
let counter_ref = Actor::spawn(Counter, 0).await?;

// Send messages
counter_ref.send(CounterMessage::Increment).await?;
```

### Named Actors (for discovery)

```rust
let counter_ref = Actor::spawn_named("global-counter", Counter, 0).await?;

// Later, find it
let found = ActorRegistry::global().find::<CounterMessage>("global-counter");
```

### Spawning with Supervision

```rust
let supervisor = Supervisor::new(SupervisorConfig {
    strategy: SupervisionStrategy::RestartChild {
        max_restarts: 5,
        within: Duration::from_secs(60),
    },
    children: vec![
        ChildSpec {
            name: "worker-1".into(),
            actor: Box::new(Worker),
            args: Box::new(WorkerArgs { id: 1 }),
            restart: RestartPolicy::Permanent,
        },
    ],
});

let supervisor_ref = Actor::spawn(supervisor, SupervisorConfig).await?;
```

---

## RPC-Style Calls

Sometimes you need a reply:

```rust
pub enum CounterMessage {
    Increment,
    Decrement,
    Get(RpcReplyPort<u64>),  // ← Reply channel
}

// Caller
let count = counter_ref.call(|reply_port| {
    CounterMessage::Get(reply_port)
}).await?;

println!("Count: {}", count);
```

**Implementation**:
```rust
pub struct RpcReplyPort<T> {
    tx: oneshot::Sender<T>,
}

impl<T> RpcReplyPort<T> {
    pub fn send(self, value: T) {
        let _ = self.tx.send(value);
    }
}
```

---

## Registry & Discovery

```rust
pub struct ActorRegistry {
    by_name: HashMap<String, ActorCell>,
    by_id: HashMap<ActorId, ActorCell>,
}

impl ActorRegistry {
    /// Get the global registry
    pub fn global() -> &'static ActorRegistry;
    
    /// Find actor by name
    pub fn find<M: Message>(&self, name: &str) -> Option<ActorRef<M>>;
    
    /// Register an actor
    pub fn register(&self, name: String, actor: ActorCell);
    
    /// Unregister (called on actor stop)
    pub fn unregister(&self, name: &str);
}
```

**Usage**:
```rust
// Register during spawn
let actor = Actor::spawn_named("my-actor", MyActor, args).await?;

// Find later
let actor_ref = ActorRegistry::global()
    .find::<MyMessage>("my-actor")
    .expect("actor not found");
```

---

## Error Handling

```rust
pub enum ActorError {
    /// Error during message processing
    ProcessingError(Box<dyn Error + Send>),
    
    /// Mailbox is full (backpressure)
    MailboxFull,
    
    /// Actor stopped
    ActorStopped,
    
    /// Serialization failed
    SerializationError,
}

// In handler
async fn handle(&self, ctx: &ActorContext<Self::Message>, msg: Msg, state: &mut State) 
    -> Result<(), ActorError> 
{
    // Errors automatically trigger supervision
    state.db.query().await
        .map_err(|e| ActorError::ProcessingError(Box::new(e)))?;
    
    Ok(())
}
```

**Supervision on Error**:
- `Err(ActorError)` → supervisor notified
- Supervisor decides: restart, escalate, or stop
- Actor's `post_stop()` called for cleanup

---

## Observability

### Tracing

```rust
#[tracing::instrument(skip(self, ctx, state))]
async fn handle(&self, ctx: &ActorContext<Self::Message>, msg: MyMessage, state: &mut State) 
    -> Result<(), ActorError> 
{
    tracing::info!("Processing message: {:?}", msg);
    // ... handle message
    Ok(())
}
```

**Built-in spans**:
- `actor.spawn` - Actor creation
- `actor.message` - Message processing
- `actor.stop` - Actor shutdown

### Metrics

```rust
// Automatically tracked:
mycelium_actor_messages_received{actor="polygon-adapter"} 12500
mycelium_actor_messages_processed{actor="polygon-adapter"} 12498
mycelium_actor_processing_duration_us{actor="polygon-adapter", p99="450"}
mycelium_actor_mailbox_size{actor="polygon-adapter"} 2
mycelium_actor_restarts{actor="polygon-adapter"} 0
```

---

## Performance Characteristics

### Message Passing Latency

| Scenario | Transport | Latency | Notes |
|----------|-----------|---------|-------|
| Same process | `Arc<T>` | ~200ns | Atomic refcount + channel send |
| Same machine | Unix socket | ~50μs | Syscall overhead, zero-copy |
| Different machine | TCP + rkyv | ~500μs | Network latency |

### Memory

- `ActorRef<M>`: 16 bytes (Arc pointer + PhantomData)
- `ActorCell`: 128 bytes (id, mailbox, supervisor, monitors)
- Message overhead: 0 bytes (Arc) or payload size (serialized)

### Throughput

**Single actor**:
- ~5M msg/sec (Arc transport, trivial processing)
- ~100K msg/sec (Unix socket, trivial processing)
- Limited by network for TCP

**System**:
- Actors run on Tokio thread pool
- Scales to # of CPU cores
- 1000+ actors per process easily

---

## Deployment Topologies

### Monolith (Development)

```toml
# config/profiles/development.toml
[deployment]
mode = "monolith"
transport = "arc"

[actors]
polygon_adapter = { bundle = "main" }
ethereum_adapter = { bundle = "main" }
flash_arbitrage = { bundle = "main" }
order_manager = { bundle = "main" }
```

**All actors in one process** → Arc transport → ultra-low latency

---

### Multi-Process (Staging)

```toml
# config/profiles/staging.toml
[deployment]
mode = "multi-process"
transport = "unix"

[bundles.data]
actors = ["polygon_adapter", "ethereum_adapter"]

[bundles.strategies]
actors = ["flash_arbitrage"]

[bundles.execution]
actors = ["order_manager"]
```

**Actors in separate processes** → Unix sockets → process isolation

---

### Distributed (Production)

```toml
# config/profiles/production.toml
[deployment]
mode = "distributed"
transport = "adaptive"  # Arc > Unix > TCP

[nodes.data-1]
host = "10.0.1.10"
actors = ["polygon_adapter"]

[nodes.data-2]
host = "10.0.1.11"
actors = ["ethereum_adapter"]

[nodes.strategy-1]
host = "10.0.2.10"
actors = ["flash_arbitrage"]

[nodes.execution-1]
host = "10.0.3.10"
actors = ["order_manager"]
```

**Actors on different machines** → TCP → horizontal scaling

---

## Example: Flash Arbitrage Actor

```rust
use mycelium_core::{Actor, ActorContext, ActorError, ActorRef, Message};
use mycelium_messages::{SwapEvent, ArbitrageSignal};

pub struct FlashArbitrageActor;

pub enum FlashArbMessage {
    SwapEvent(SwapEvent),
    UpdateConfig(ArbConfig),
}

impl Message for FlashArbMessage {}

pub struct FlashArbState {
    pools: HashMap<Address, Pool>,
    signals_sent: u64,
}

#[async_trait]
impl Actor for FlashArbitrageActor {
    type Message = FlashArbMessage;
    type State = FlashArbState;
    type Arguments = ();
    
    async fn pre_start(&self, _ctx: &ActorContext<Self::Message>, _args: ()) 
        -> Result<Self::State, ActorError> 
    {
        Ok(FlashArbState {
            pools: HashMap::new(),
            signals_sent: 0,
        })
    }
    
    #[tracing::instrument(skip(self, ctx, state))]
    async fn handle(
        &self,
        ctx: &ActorContext<Self::Message>,
        message: Self::Message,
        state: &mut Self::State,
    ) -> Result<(), ActorError> {
        match message {
            FlashArbMessage::SwapEvent(swap) => {
                // Update pool state
                state.pools.entry(swap.pool_address)
                    .and_modify(|pool| pool.update_reserves(swap.reserves));
                
                // Detect arbitrage opportunities
                if let Some(opportunity) = self.detect_arbitrage(&state.pools) {
                    // Send signal to order manager
                    if let Some(order_mgr) = ctx.find::<OrderMessage>("order-manager") {
                        let signal = ArbitrageSignal {
                            opportunity_id: state.signals_sent,
                            path: opportunity.path,
                            estimated_profit: opportunity.profit,
                        };
                        
                        order_mgr.send(OrderMessage::ArbitrageSignal(signal)).await?;
                        state.signals_sent += 1;
                    }
                }
            }
            FlashArbMessage::UpdateConfig(config) => {
                tracing::info!("Config updated: {:?}", config);
            }
        }
        Ok(())
    }
}
```

---

## Migration from Torq

### Torq's Actor Pattern

```rust
// Torq (simplified)
async fn polygon_composite_service() {
    let (tx, mut rx) = mpsc::channel(1000);
    
    while let Some(event) = rx.recv().await {
        // Process event
        handle_event(event).await;
    }
}
```

**Issues**:
- Manual channel management
- No supervision
- No discovery
- Hard to test

### Mycelium Actor

```rust
struct PolygonComposite;

#[async_trait]
impl Actor for PolygonComposite {
    type Message = BlockchainEvent;
    type State = CompositeState;
    type Arguments = CompositeConfig;
    
    async fn handle(&self, ctx: &ActorContext<Self::Message>, event: BlockchainEvent, state: &mut State) 
        -> Result<(), ActorError> 
    {
        // Same logic, but now:
        // ✅ Supervised
        // ✅ Discoverable
        // ✅ Testable
        // ✅ Observable
        handle_event(ctx, event, state).await
    }
}
```

---

## Testing Actors

### Unit Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use mycelium_testkit::ActorTestHarness;
    
    #[tokio::test]
    async fn test_counter_increment() {
        let harness = ActorTestHarness::new(Counter, 0).await;
        
        // Send message
        harness.send(CounterMessage::Increment).await;
        
        // Check state
        let count = harness.call(|reply| CounterMessage::Get(reply)).await;
        assert_eq!(count, 1);
    }
    
    #[tokio::test]
    async fn test_counter_handles_errors() {
        let harness = ActorTestHarness::new(Counter, 0).await;
        
        // Inject error
        harness.send(CounterMessage::Fail).await;
        
        // Verify supervision kicked in
        assert!(harness.was_restarted());
    }
}
```

### Integration Testing

```rust
#[tokio::test]
async fn test_arbitrage_pipeline() {
    // Spawn actors
    let adapter = Actor::spawn_named("adapter", PolygonAdapter, config).await?;
    let strategy = Actor::spawn_named("strategy", FlashArb, ()).await?;
    
    // Inject test event
    adapter.send(TestEvent::Swap { ... }).await?;
    
    // Verify strategy received and processed
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    let signals_sent = strategy.call(|reply| FlashArbMessage::GetStats(reply)).await?;
    assert_eq!(signals_sent, 1);
}
```

---

## Summary

**What We're Building**:
- ✅ Erlang-style actor system on Tokio
- ✅ Adaptive transport (Arc → Unix → TCP)
- ✅ Type-safe message passing
- ✅ Zero-copy where possible
- ✅ Supervision trees for fault tolerance
- ✅ Built-in observability

**What We're NOT Building**:
- ❌ Full Erlang/OTP clone (too complex)
- ❌ Another web framework (use actix-web if needed)
- ❌ Generic distributed system (focus on HFT)

**Estimated Effort**:
- Week 1-2: Core actor runtime + supervision (MVP)
- Week 3-4: Arc transport + registry
- Week 5-6: Serialization (rkyv)
- Week 7-8: Unix socket transport
- Week 9-10: TCP transport + discovery
- Week 11-12: Hardening + docs

**Next Steps**:
1. Review this design
2. Prototype core Actor trait + basic runtime
3. Benchmark Arc transport vs raw channels
4. Validate assumptions before full build

---

## Open Questions

1. **Mailbox capacity**: Default 1000? Configurable per-actor?
2. **Message priorities**: Do we need more than 2 levels (system/user)?
3. **Actor discovery**: DNS-based for distributed? Or custom protocol?
4. **Serialization format**: rkyv only? Or support multiple (protobuf, bincode)?
5. **Hot code reload**: Worth supporting? (Erlang does this, very complex)

**Feedback welcome!** 🦀
