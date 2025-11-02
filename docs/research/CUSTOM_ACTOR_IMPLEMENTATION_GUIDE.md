# Custom Actor Implementation Guide for Mycelium

**Quick Reference**: How to build Mycelium's actor system on Tokio

---

## Why Custom Implementation?

1. **Adaptive transport requirement** - No framework supports Arc<T> → Unix → TCP
2. **HFT latency targets** - Need < 500ns local messaging
3. **Zero-copy optimization** - Full control over serialization paths
4. **Production HFT pattern** - Industry standard is custom implementations

---

## Core Architecture

### Three-Layer Design

```
┌─────────────────────────────────────────────────┐
│           Application Layer                     │
│  (Trading strategies, market data, execution)   │
└─────────────────────────────────────────────────┘
                      ↓
┌─────────────────────────────────────────────────┐
│          Actor Abstraction Layer                │
│   ActorRef<T> - Unified API for all transports │
└─────────────────────────────────────────────────┘
                      ↓
┌─────────────────────────────────────────────────┐
│           Transport Layer                       │
│  Local (Arc) | Unix Socket | TCP               │
└─────────────────────────────────────────────────┘
                      ↓
┌─────────────────────────────────────────────────┐
│           Tokio Runtime                         │
└─────────────────────────────────────────────────┘
```

---

## Core Pattern: Task + Handle

**Based on Alice Ryhl's canonical pattern** - https://ryhl.io/blog/actors-with-tokio/

```rust
use tokio::sync::mpsc;

// The actor's internal state and message receiver
struct MyActor {
    receiver: mpsc::Receiver<MyMessage>,
    state: ActorState,
}

// The handle that others use to communicate with the actor
#[derive(Clone)]
pub struct MyActorHandle {
    sender: mpsc::Sender<MyMessage>,
}

// Message types the actor handles
pub enum MyMessage {
    DoSomething(String),
    GetState(oneshot::Sender<ActorState>),
    Shutdown,
}

impl MyActor {
    fn new(receiver: mpsc::Receiver<MyMessage>) -> Self {
        Self {
            receiver,
            state: ActorState::default(),
        }
    }

    // The actor's main loop
    async fn run(mut self) {
        while let Some(msg) = self.receiver.recv().await {
            match msg {
                MyMessage::DoSomething(data) => {
                    self.handle_do_something(data).await;
                }
                MyMessage::GetState(reply) => {
                    let _ = reply.send(self.state.clone());
                }
                MyMessage::Shutdown => {
                    println!("Shutting down actor");
                    break;
                }
            }
        }
    }

    async fn handle_do_something(&mut self, data: String) {
        // Actor logic here
        println!("Processing: {}", data);
    }
}

impl MyActorHandle {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel(32); // Bounded mailbox
        let actor = MyActor::new(receiver);
        
        // Spawn the actor task
        tokio::spawn(async move {
            actor.run().await;
        });
        
        Self { sender }
    }

    pub async fn do_something(&self, data: String) -> Result<(), SendError> {
        self.sender.send(MyMessage::DoSomething(data)).await
            .map_err(|_| SendError::ActorDead)
    }

    pub async fn get_state(&self) -> Result<ActorState, SendError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender.send(MyMessage::GetState(reply_tx)).await
            .map_err(|_| SendError::ActorDead)?;
        reply_rx.await.map_err(|_| SendError::ActorDead)
    }

    pub async fn shutdown(&self) -> Result<(), SendError> {
        self.sender.send(MyMessage::Shutdown).await
            .map_err(|_| SendError::ActorDead)
    }
}
```

---

## Mycelium Extensions

### 1. Adaptive Transport ActorRef

```rust
use std::sync::Arc;

/// Universal actor reference supporting multiple transports
pub enum ActorRef<T: Message> {
    /// Same process - zero-copy via Arc
    Local(LocalActorRef<T>),
    
    /// Same machine - Unix domain socket
    UnixSocket(UnixSocketActorRef<T>),
    
    /// Remote machine - TCP
    Tcp(TcpActorRef<T>),
}

impl<T: Message> ActorRef<T> {
    /// Send message - API same regardless of transport
    pub async fn send(&self, msg: T) -> Result<()> {
        match self {
            Self::Local(r) => r.send_local(msg).await,
            Self::UnixSocket(r) => r.send_serialized(msg).await,
            Self::Tcp(r) => r.send_serialized(msg).await,
        }
    }

    /// Send with timeout
    pub async fn send_timeout(&self, msg: T, timeout: Duration) -> Result<()> {
        tokio::time::timeout(timeout, self.send(msg))
            .await
            .map_err(|_| Error::Timeout)?
    }

    /// Try send (non-blocking, for backpressure)
    pub fn try_send(&self, msg: T) -> Result<()> {
        match self {
            Self::Local(r) => r.try_send(msg),
            Self::UnixSocket(r) => r.try_send(msg),
            Self::Tcp(r) => r.try_send(msg),
        }
    }
}

/// Local actor reference - fastest path
pub struct LocalActorRef<T> {
    sender: mpsc::Sender<Arc<T>>,
    actor_id: ActorId,
}

impl<T: Message> LocalActorRef<T> {
    async fn send_local(&self, msg: T) -> Result<()> {
        // Zero-copy: wrap in Arc, no serialization
        self.sender.send(Arc::new(msg)).await
            .map_err(|_| Error::ActorDead)
    }
}

/// Unix socket actor reference - same machine
pub struct UnixSocketActorRef<T> {
    connection: Arc<UnixConnection>,
    actor_id: ActorId,
    _phantom: PhantomData<T>,
}

impl<T: Message> UnixSocketActorRef<T> {
    async fn send_serialized(&self, msg: T) -> Result<()> {
        // Serialize once with rkyv
        let bytes = msg.serialize_rkyv()?;
        
        // Send over Unix socket
        self.connection.send(&bytes).await?;
        Ok(())
    }
}

/// TCP actor reference - remote machine
pub struct TcpActorRef<T> {
    connection: Arc<TcpConnection>,
    actor_id: ActorId,
    _phantom: PhantomData<T>,
}

impl<T: Message> TcpActorRef<T> {
    async fn send_serialized(&self, msg: T) -> Result<()> {
        // Serialize with rkyv + framing
        let bytes = msg.serialize_rkyv()?;
        
        // Send over TCP
        self.connection.send(&bytes).await?;
        Ok(())
    }
}
```

### 2. Message Trait for Zero-Copy

```rust
use rkyv::{Archive, Serialize, Deserialize};

/// Messages must support both Arc<T> and rkyv serialization
pub trait Message: Send + Sync + 'static + Archive + Serialize<AllocSerializer<256>> {
    /// Serialize using rkyv for zero-copy deserialization
    fn serialize_rkyv(&self) -> Result<Vec<u8>> {
        rkyv::to_bytes(self)
            .map(|v| v.to_vec())
            .map_err(|e| Error::SerializationFailed(e.to_string()))
    }

    /// Deserialize from rkyv bytes (zero-copy)
    fn deserialize_rkyv(bytes: &[u8]) -> Result<&Self::Archived> {
        rkyv::check_archived_root::<Self>(bytes)
            .map_err(|e| Error::DeserializationFailed(e.to_string()))
    }
}

// Example usage
#[derive(Archive, Serialize, Deserialize)]
#[archive(check_bytes)]
pub struct MarketTick {
    pub symbol: String,
    pub price: f64,
    pub volume: u64,
    pub timestamp: u64,
}

impl Message for MarketTick {}

// Local messaging (zero serialization)
let tick = MarketTick { /* ... */ };
local_ref.send(tick).await?;  // Wraps in Arc<T>

// Remote messaging (zero-copy deserialize)
let tick = MarketTick { /* ... */ };
unix_ref.send(tick).await?;   // Serializes once, receiver gets Archived<MarketTick>
```

### 3. Supervision Tree (Erlang-Style)

```rust
/// Supervisor manages child actors and restarts them on failure
pub struct Supervisor {
    children: HashMap<ActorId, ChildSpec>,
    strategy: SupervisionStrategy,
}

pub struct ChildSpec {
    actor_id: ActorId,
    spawn_fn: Box<dyn Fn() -> ActorRef<dyn Any> + Send>,
    restart_strategy: RestartStrategy,
    join_handle: Option<JoinHandle<()>>,
}

pub enum SupervisionStrategy {
    /// Restart only the failed child
    OneForOne,
    
    /// Restart all children if one fails
    OneForAll,
    
    /// Restart failed child and all started after it
    RestForOne,
}

pub enum RestartStrategy {
    /// Always restart on failure
    Permanent,
    
    /// Restart only on abnormal exit
    Transient,
    
    /// Never restart
    Temporary,
}

impl Supervisor {
    pub async fn run(mut self) {
        loop {
            // Monitor all child tasks
            let mut futures: Vec<_> = self.children.values_mut()
                .filter_map(|child| child.join_handle.as_mut())
                .collect();

            // Wait for any child to exit
            let (result, index, _remaining) = select_all(futures).await;

            match result {
                Ok(_) => {
                    // Normal exit
                    println!("Child exited normally");
                }
                Err(e) if e.is_panic() => {
                    // Child panicked - apply restart strategy
                    println!("Child panicked, restarting...");
                    self.handle_child_failure(index).await;
                }
                Err(_) => {
                    // Child cancelled
                    println!("Child cancelled");
                }
            }
        }
    }

    async fn handle_child_failure(&mut self, failed_index: usize) {
        match self.strategy {
            SupervisionStrategy::OneForOne => {
                self.restart_child(failed_index).await;
            }
            SupervisionStrategy::OneForAll => {
                self.restart_all_children().await;
            }
            SupervisionStrategy::RestForOne => {
                self.restart_from_index(failed_index).await;
            }
        }
    }

    async fn restart_child(&mut self, index: usize) {
        let child = &self.children.values().nth(index).unwrap();
        
        match child.restart_strategy {
            RestartStrategy::Permanent | RestartStrategy::Transient => {
                // Respawn the actor
                let new_ref = (child.spawn_fn)();
                println!("Restarted actor: {:?}", child.actor_id);
            }
            RestartStrategy::Temporary => {
                println!("Not restarting temporary actor: {:?}", child.actor_id);
            }
        }
    }
}
```

### 4. Actor Registry

```rust
use dashmap::DashMap;

/// Thread-safe actor registry
pub struct ActorRegistry {
    actors: Arc<DashMap<ActorId, Box<dyn Any + Send + Sync>>>,
}

impl ActorRegistry {
    pub fn new() -> Self {
        Self {
            actors: Arc::new(DashMap::new()),
        }
    }

    /// Register an actor
    pub fn register<T: Message>(&self, id: ActorId, actor_ref: ActorRef<T>) {
        self.actors.insert(id, Box::new(actor_ref));
    }

    /// Lookup an actor by ID
    pub fn lookup<T: Message>(&self, id: &ActorId) -> Option<ActorRef<T>> {
        self.actors.get(id).and_then(|entry| {
            let boxed = entry.value();
            // Downcast to concrete type
            boxed.downcast_ref::<ActorRef<T>>().cloned()
        })
    }

    /// Remove an actor
    pub fn unregister(&self, id: &ActorId) {
        self.actors.remove(id);
    }

    /// List all actors
    pub fn list(&self) -> Vec<ActorId> {
        self.actors.iter().map(|entry| entry.key().clone()).collect()
    }
}

// Global registry
lazy_static! {
    pub static ref REGISTRY: ActorRegistry = ActorRegistry::new();
}

// Usage
REGISTRY.register(actor_id, actor_ref);
let actor = REGISTRY.lookup::<MyMessage>(&actor_id).ok_or(Error::ActorNotFound)?;
actor.send(msg).await?;
```

---

## Performance Optimizations

### 1. Hot Path: Direct Channels

```rust
/// For ultra-low-latency actors, skip ActorRef abstraction
pub struct HotPathActor {
    // Direct access to Tokio channel
    rx: mpsc::Receiver<Arc<HotPathMessage>>,
}

pub struct HotPathHandle {
    // Direct Tokio sender (no enum, no dynamic dispatch)
    tx: mpsc::Sender<Arc<HotPathMessage>>,
}

// Usage: ~200ns latency (Tokio baseline)
handle.tx.send(Arc::new(msg)).await?;
```

### 2. Bounded Mailboxes

```rust
// Small bounded mailbox for backpressure
let (tx, rx) = mpsc::channel(32);  // Only 32 messages queued

// Send blocks when full (applies backpressure)
tx.send(msg).await?;

// Or fail fast
match tx.try_send(msg) {
    Ok(_) => {},
    Err(TrySendError::Full(_)) => {
        // Actor overwhelmed, trigger circuit breaker
        circuit_breaker.trip();
    }
    Err(TrySendError::Closed(_)) => {
        // Actor dead
        return Err(Error::ActorDead);
    }
}
```

### 3. Zero-Copy Local Messaging

```rust
// Sender
let msg = MarketTick { /* ... */ };
let arc_msg = Arc::new(msg);

// Send to multiple actors without cloning data
for actor in &subscribers {
    actor.send(Arc::clone(&arc_msg)).await?;  // Only clone Arc pointer
}

// Receiver
async fn handle(&mut self, msg: Arc<MarketTick>) {
    // Access data without ownership
    println!("Price: {}", msg.price);
    
    // Can hold Arc across await points
    self.process(Arc::clone(&msg)).await;
}
```

### 4. rkyv Zero-Copy Deserialization

```rust
// Sender (Unix socket or TCP)
let msg = MarketTick { /* ... */ };
let bytes = rkyv::to_bytes(&msg)?;
socket.send(&bytes).await?;

// Receiver
let bytes = socket.recv().await?;

// Zero-copy access (no deserialization)
let archived = rkyv::check_archived_root::<MarketTick>(&bytes)?;

// Access fields directly from bytes
println!("Price: {}", archived.price);  // No allocation!

// Optional: deserialize if mutation needed
let owned: MarketTick = archived.deserialize(&mut Infallible)?;
```

---

## Common Patterns

### 1. Request-Reply

```rust
pub enum MyMessage {
    GetData(oneshot::Sender<Data>),
}

// Sender
let (reply_tx, reply_rx) = oneshot::channel();
actor.send(MyMessage::GetData(reply_tx)).await?;
let data = reply_rx.await?;

// Actor
MyMessage::GetData(reply) => {
    let data = self.get_data();
    let _ = reply.send(data);  // Ignore if requester dropped
}
```

### 2. Fire-and-Forget

```rust
pub enum MyMessage {
    Notify(String),
}

// Sender
actor.send(MyMessage::Notify("event".into())).await?;

// Actor
MyMessage::Notify(event) => {
    self.handle_notification(event);
}
```

### 3. Broadcast to Subscribers

```rust
struct BroadcastActor {
    subscribers: Vec<ActorRef<SubscriberMessage>>,
}

impl BroadcastActor {
    async fn broadcast(&self, data: Arc<Data>) {
        // Clone Arc (cheap), not data
        for subscriber in &self.subscribers {
            let _ = subscriber.send(Arc::clone(&data)).await;
        }
    }
}
```

### 4. Graceful Shutdown

```rust
pub enum MyMessage {
    Shutdown,
}

async fn run(mut self) {
    while let Some(msg) = self.receiver.recv().await {
        match msg {
            MyMessage::Shutdown => {
                self.cleanup().await;
                break;
            }
            // ... other messages
        }
    }
}

// Or use CancellationToken
use tokio_util::sync::CancellationToken;

async fn run(mut self, cancel: CancellationToken) {
    loop {
        tokio::select! {
            Some(msg) = self.receiver.recv() => {
                self.handle(msg).await;
            }
            _ = cancel.cancelled() => {
                self.cleanup().await;
                break;
            }
        }
    }
}
```

---

## Critical Lessons (From Research)

### 1. Avoid Cycles with Bounded Channels

```rust
// BAD: Deadlock if both mailboxes full
Actor A --[bounded(32)]--> Actor B
Actor B --[bounded(32)]--> Actor A

// GOOD: Break cycle with unbounded or try_send
Actor A --[bounded(32)]--> Actor B
Actor B --[unbounded()]--> Actor A

// BETTER: Avoid cycles entirely
Actor A --[bounded(32)]--> Actor B
Actor B --[bounded(32)]--> Actor C (no cycle)
```

### 2. Don't Block the Event Loop

```rust
// BAD: Blocks Tokio thread
async fn handle(&mut self, msg: Message) {
    let result = heavy_computation();  // Blocks!
}

// GOOD: Offload to rayon
use rayon::prelude::*;

async fn handle(&mut self, msg: Message) {
    let result = tokio::task::spawn_blocking(move || {
        heavy_computation()  // Runs on thread pool
    }).await?;
}
```

### 3. Bounded Mailboxes Prevent Memory Leaks

```rust
// BAD: Unbounded can grow infinitely
let (tx, rx) = mpsc::unbounded_channel();

// GOOD: Bounded prevents runaway queues
let (tx, rx) = mpsc::channel(32);

// BETTER: Monitor queue depth
if tx.capacity() < 8 {
    metrics.record_backpressure();
    circuit_breaker.consider_tripping();
}
```

### 4. Use try_send for Critical Actors

```rust
// GOOD: Fail fast instead of blocking
match actor.try_send(msg) {
    Ok(_) => {},
    Err(TrySendError::Full(_)) => {
        // Actor overwhelmed - log and move on
        warn!("Actor mailbox full, dropping message");
        metrics.increment_dropped_messages();
    }
    Err(TrySendError::Closed(_)) => {
        // Actor dead - restart via supervisor
        supervisor.restart_actor(actor_id);
    }
}
```

---

## Testing Strategies

### 1. Unit Testing Actors

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn test_actor_processes_message() {
        let handle = MyActorHandle::new();
        
        // Send message
        handle.do_something("test".into()).await.unwrap();
        
        // Verify state
        let state = handle.get_state().await.unwrap();
        assert_eq!(state.count, 1);
        
        // Cleanup
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_actor_backpressure() {
        let handle = MyActorHandle::new();
        
        // Fill mailbox
        for i in 0..32 {
            handle.do_something(format!("msg{}", i)).await.unwrap();
        }
        
        // Next send should block or fail
        let result = timeout(
            Duration::from_millis(100),
            handle.do_something("overflow".into())
        ).await;
        
        assert!(result.is_err(), "Should timeout due to backpressure");
    }
}
```

### 2. Testing Supervision

```rust
#[tokio::test]
async fn test_supervisor_restarts_failed_actor() {
    let supervisor = Supervisor::new(SupervisionStrategy::OneForOne);
    
    // Add child that will panic
    let child_id = supervisor.add_child(|| {
        // Actor that panics on first message
        PanickyActorHandle::new()
    }, RestartStrategy::Permanent);
    
    // Send message that causes panic
    supervisor.children[&child_id].send_panic_trigger().await.unwrap();
    
    // Wait for restart
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Verify child restarted
    assert!(supervisor.is_child_alive(&child_id));
}
```

### 3. Testing Different Transports

```rust
#[tokio::test]
async fn test_local_transport() {
    let actor = spawn_local_actor();
    let msg = TestMessage { data: "test".into() };
    
    actor.send(msg).await.unwrap();
    // Verify via local channels
}

#[tokio::test]
async fn test_unix_socket_transport() {
    let actor = spawn_unix_actor("/tmp/test.sock").await.unwrap();
    let msg = TestMessage { data: "test".into() };
    
    actor.send(msg).await.unwrap();
    // Verify via Unix socket
}

#[tokio::test]
async fn test_tcp_transport() {
    let actor = spawn_tcp_actor("127.0.0.1:8080").await.unwrap();
    let msg = TestMessage { data: "test".into() };
    
    actor.send(msg).await.unwrap();
    // Verify via TCP
}
```

---

## Benchmarking

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark_local_messaging(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    c.bench_function("local_actor_send", |b| {
        b.to_async(&rt).iter(|| async {
            let handle = MyActorHandle::new();
            let msg = TestMessage { data: "test".into() };
            
            black_box(handle.send(msg).await.unwrap());
        });
    });
}

fn benchmark_rkyv_serialization(c: &mut Criterion) {
    c.bench_function("rkyv_serialize", |b| {
        let msg = TestMessage { data: "test".into() };
        
        b.iter(|| {
            black_box(rkyv::to_bytes(&msg).unwrap());
        });
    });
    
    c.bench_function("rkyv_deserialize", |b| {
        let msg = TestMessage { data: "test".into() };
        let bytes = rkyv::to_bytes(&msg).unwrap();
        
        b.iter(|| {
            black_box(rkyv::check_archived_root::<TestMessage>(&bytes).unwrap());
        });
    });
}

criterion_group!(benches, benchmark_local_messaging, benchmark_rkyv_serialization);
criterion_main!(benches);
```

---

## Next Steps

1. **Prototype basic Tokio actor** (task + handle pattern)
2. **Add supervision tree** (one-for-one restart strategy)
3. **Implement LocalActorRef** with Arc<T> messaging
4. **Benchmark against Actix** to validate performance
5. **Add rkyv serialization** for Message trait
6. **Implement UnixSocketActorRef** for multi-process
7. **Implement TcpActorRef** for distributed
8. **Build actor registry** with discovery
9. **Add monitoring/observability** hooks
10. **Production hardening** (circuit breakers, graceful shutdown)

---

## Resources

- **Tokio Actors Tutorial**: https://ryhl.io/blog/actors-with-tokio/
- **Tokio Channels**: https://tokio.rs/tokio/tutorial/channels
- **rkyv Documentation**: https://rkyv.org/
- **Ractor (for supervision patterns)**: https://slawlor.github.io/ractor/
- **Rust Serialization Benchmarks**: https://github.com/djkoloski/rust_serialization_benchmark

---

**This guide provides the foundation for building Mycelium's actor system. Start simple, benchmark continuously, and optimize based on profiling data.**
