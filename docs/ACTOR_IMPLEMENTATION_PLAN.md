# Actor System Implementation Plan

**Goal**: Build location-transparent actor system with adaptive transport in 2-week sprint.

**Philosophy**: Spec → Contract Tests → Implementation

---

## Week 1: Core Actor Abstraction

### Day 1-2: System Spec (GUARDRAILS)

**Deliverable**: `docs/GUARDRAILS/system.yaml` defining actor system architecture

```yaml
# docs/GUARDRAILS/system.yaml
version: "0.1.0"
last_updated: "2025-11-02"

system:
  name: "Mycelium Actor Runtime"
  description: "Location-transparent actor system with adaptive transport"

components:
  actor_core:
    description: "Core actor abstractions"
    responsibilities:
      - "Define Actor trait with lifecycle hooks"
      - "Manage actor state transitions"
      - "Handle message routing"
    guarantees:
      - "Actors process messages sequentially (no internal concurrency)"
      - "Messages delivered in FIFO order per sender"
      - "Failed actors can be restarted by supervisor"

  mailbox:
    description: "Actor message queue"
    responsibilities:
      - "Buffer incoming messages"
      - "Priority queue support (Normal, High)"
      - "Backpressure signaling when full"
    guarantees:
      - "Bounded capacity (configurable)"
      - "Messages never lost in steady state"
      - "Priority ordering within queue"

  actor_ref:
    description: "Location-transparent actor reference"
    responsibilities:
      - "Abstract over actor location (local/remote)"
      - "Select transport based on deployment"
      - "Handle send failures gracefully"
    guarantees:
      - "ActorRef is Send + Sync + Clone"
      - "Sends never block caller (async)"
      - "Delivery failures are observable"

  supervision:
    description: "Actor failure handling"
    strategies:
      - "OneForOne: restart only failed actor"
      - "OneForAll: restart all supervised actors"
      - "RestForOne: restart failed + subsequent actors"
    guarantees:
      - "Max restart attempts enforced"
      - "Restart backoff prevents thrashing"
      - "Supervisor never panics on child failure"

transport:
  local:
    mechanism: "Arc<Mutex<VecDeque<M>>>"
    latency: "<100ns"
    serialization: "None (zero-copy)"
    use_when: "Same process"

  unix_socket:
    mechanism: "Unix domain socket + bincode"
    latency: "<35μs"
    serialization: "bincode"
    use_when: "Same machine, different process"

  tcp:
    mechanism: "TCP socket + bincode"
    latency: "<5ms"
    serialization: "bincode"
    use_when: "Different machines"

messages:
  envelope:
    fields:
      - "actor_id: ActorId"
      - "message: M"
      - "priority: Priority"
      - "sender: Option<ActorRef>"

invariants:
  - name: "Sequential processing"
    description: "Actor processes one message at a time"
    enforced_by: ["mailbox single-consumer"]

  - name: "FIFO ordering"
    description: "Messages from same sender delivered in order"
    enforced_by: ["mailbox queue discipline"]

  - name: "No message loss"
    description: "Messages never dropped in steady state"
    enforced_by: ["bounded mailbox with backpressure"]

  - name: "Restart transparency"
    description: "ActorRef remains valid after restart"
    enforced_by: ["supervisor maintains ref mapping"]
```

**Contract Tests** (write before implementation):
```rust
// tests/contract/actor_core.rs

#[test]
fn actor_ref_is_send_sync_clone() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    fn assert_clone<T: Clone>() {}

    assert_send::<ActorRef<String>>();
    assert_sync::<ActorRef<String>>();
    assert_clone::<ActorRef<String>>();
}

#[test]
fn messages_delivered_in_fifo_order() {
    // Define expected behavior
    // Implementation will make this pass
}

#[test]
fn actor_processes_messages_sequentially() {
    // No concurrent message processing
}
```

### Day 3-4: Core Traits

**File**: `crates/mycelium-core/src/actor.rs`

```rust
use async_trait::async_trait;
use std::fmt::Debug;

/// Core actor trait.
///
/// Actors process messages sequentially and maintain internal state.
/// The runtime guarantees:
/// - Messages delivered in FIFO order (per sender)
/// - No concurrent message processing
/// - Supervisor can restart failed actors
#[async_trait]
pub trait Actor: Send + 'static {
    /// Message type this actor handles
    type Message: Send + Debug + 'static;

    /// Handle incoming message
    async fn handle(&mut self, msg: Self::Message, ctx: &mut ActorContext<Self>);

    /// Called when actor starts (before processing messages)
    async fn on_start(&mut self, _ctx: &mut ActorContext<Self>) {}

    /// Called when actor stops (after processing all messages)
    async fn on_stop(&mut self) {}

    /// Called when actor is about to restart (after failure)
    async fn on_restart(&mut self, _ctx: &mut ActorContext<Self>) {}
}

/// Actor execution context
pub struct ActorContext<A: Actor> {
    actor_id: ActorId,
    mailbox: Mailbox<A::Message>,
    supervisor: Option<SupervisorRef>,
}

impl<A: Actor> ActorContext<A> {
    /// Get this actor's unique identifier
    pub fn actor_id(&self) -> ActorId {
        self.actor_id
    }

    /// Stop this actor after processing current message
    pub fn stop(&mut self) {
        // Implementation
    }

    /// Spawn a new child actor under supervision
    pub fn spawn<B: Actor>(&mut self, actor: B) -> ActorRef<B::Message> {
        // Implementation
    }
}

/// Unique actor identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActorId(u64);

/// Location-transparent actor reference
#[derive(Clone)]
pub struct ActorRef<M> {
    actor_id: ActorId,
    transport: Transport<M>,
}

impl<M: Send + Debug + 'static> ActorRef<M> {
    /// Send message to actor (non-blocking)
    pub async fn send(&self, msg: M) -> Result<(), SendError> {
        self.transport.send(self.actor_id, msg).await
    }

    /// Send message with priority
    pub async fn send_priority(&self, msg: M, priority: Priority) -> Result<(), SendError> {
        self.transport.send_priority(self.actor_id, msg, priority).await
    }
}

/// Message priority
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Normal = 0,
    High = 1,
}

/// Send error types
#[derive(Debug, thiserror::Error)]
pub enum SendError {
    #[error("Actor mailbox full")]
    MailboxFull,

    #[error("Actor not found: {0:?}")]
    ActorNotFound(ActorId),

    #[error("Transport error: {0}")]
    Transport(String),
}
```

**Tests**:
```rust
// crates/mycelium-core/src/actor_test.rs

#[cfg(test)]
mod tests {
    use super::*;

    struct TestActor {
        messages: Vec<String>,
    }

    #[async_trait]
    impl Actor for TestActor {
        type Message = String;

        async fn handle(&mut self, msg: String, _ctx: &mut ActorContext<Self>) {
            self.messages.push(msg);
        }
    }

    #[tokio::test]
    async fn actor_processes_messages_sequentially() {
        // Spawn actor
        // Send 3 messages
        // Assert messages vector has 3 items in order
    }

    #[tokio::test]
    async fn actor_lifecycle_hooks_called() {
        // Test on_start, on_stop called
    }
}
```

### Day 5-6: Mailbox Implementation

**File**: `crates/mycelium-core/src/mailbox.rs`

```rust
use std::collections::VecDeque;
use tokio::sync::{mpsc, oneshot};

/// Actor mailbox (bounded message queue)
pub struct Mailbox<M> {
    queue: VecDeque<Envelope<M>>,
    capacity: usize,
    rx: mpsc::Receiver<Envelope<M>>,
    tx: mpsc::Sender<Envelope<M>>,
}

struct Envelope<M> {
    message: M,
    priority: Priority,
    sender: Option<ActorId>,
}

impl<M> Mailbox<M> {
    pub fn new(capacity: usize) -> Self {
        let (tx, rx) = mpsc::channel(capacity);
        Self {
            queue: VecDeque::new(),
            capacity,
            rx,
            tx,
        }
    }

    /// Receive next message (blocks if empty)
    pub async fn recv(&mut self) -> Option<Envelope<M>> {
        self.rx.recv().await
    }

    /// Send message to mailbox
    pub async fn send(&self, msg: M, priority: Priority) -> Result<(), SendError> {
        let envelope = Envelope {
            message: msg,
            priority,
            sender: None,
        };

        self.tx.send(envelope).await
            .map_err(|_| SendError::MailboxFull)
    }

    /// Get mailbox handle (for ActorRef)
    pub fn handle(&self) -> MailboxHandle<M> {
        MailboxHandle {
            tx: self.tx.clone(),
        }
    }
}

/// Clonable mailbox handle (stored in ActorRef)
#[derive(Clone)]
pub struct MailboxHandle<M> {
    tx: mpsc::Sender<Envelope<M>>,
}

impl<M> MailboxHandle<M> {
    pub async fn send(&self, msg: M, priority: Priority) -> Result<(), SendError> {
        let envelope = Envelope {
            message: msg,
            priority,
            sender: None,
        };

        self.tx.send(envelope).await
            .map_err(|_| SendError::MailboxFull)
    }
}
```

**Tests**:
```rust
#[tokio::test]
async fn mailbox_fifo_ordering() {
    let mut mailbox = Mailbox::<String>::new(10);
    let handle = mailbox.handle();

    // Send 3 messages
    handle.send("msg1".into(), Priority::Normal).await.unwrap();
    handle.send("msg2".into(), Priority::Normal).await.unwrap();
    handle.send("msg3".into(), Priority::Normal).await.unwrap();

    // Receive in order
    assert_eq!(mailbox.recv().await.unwrap().message, "msg1");
    assert_eq!(mailbox.recv().await.unwrap().message, "msg2");
    assert_eq!(mailbox.recv().await.unwrap().message, "msg3");
}

#[tokio::test]
async fn mailbox_priority_ordering() {
    // High priority messages jump queue
}

#[tokio::test]
async fn mailbox_backpressure() {
    // Send fails when mailbox full
}
```

### Day 7: Integration

**File**: `crates/mycelium-core/src/runtime.rs`

```rust
/// Actor runtime (spawns and manages actors)
pub struct Runtime {
    actors: HashMap<ActorId, Box<dyn ActorHandle>>,
    next_id: AtomicU64,
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            actors: HashMap::new(),
            next_id: AtomicU64::new(1),
        }
    }

    /// Spawn actor and return reference
    pub fn spawn<A: Actor>(&mut self, actor: A) -> ActorRef<A::Message> {
        let actor_id = ActorId(self.next_id.fetch_add(1, Ordering::SeqCst));
        let mailbox = Mailbox::new(100); // TODO: configurable
        let handle = mailbox.handle();

        // Spawn task to run actor
        let task = tokio::spawn(Self::run_actor(actor_id, actor, mailbox));

        // Store handle
        self.actors.insert(actor_id, Box::new(task));

        // Return ActorRef
        ActorRef {
            actor_id,
            transport: Transport::Local(handle),
        }
    }

    async fn run_actor<A: Actor>(
        actor_id: ActorId,
        mut actor: A,
        mut mailbox: Mailbox<A::Message>,
    ) {
        let mut ctx = ActorContext {
            actor_id,
            mailbox: mailbox.clone(),
            supervisor: None,
        };

        // Call on_start
        actor.on_start(&mut ctx).await;

        // Process messages
        while let Some(envelope) = mailbox.recv().await {
            actor.handle(envelope.message, &mut ctx).await;
        }

        // Call on_stop
        actor.on_stop().await;
    }
}
```

**Integration test**:
```rust
// tests/integration/ping_pong.rs

struct PingActor {
    count: usize,
    pong_ref: Option<ActorRef<Pong>>,
}

struct PongActor {
    count: usize,
}

enum Ping {
    Start(ActorRef<Pong>),
    Pong,
}

enum Pong {
    Ping,
}

#[tokio::test]
async fn ping_pong_actors() {
    let mut runtime = Runtime::new();

    let ping = runtime.spawn(PingActor { count: 0, pong_ref: None });
    let pong = runtime.spawn(PongActor { count: 0 });

    ping.send(Ping::Start(pong.clone())).await.unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Assert messages exchanged
}
```

---

## Week 2: Transport Abstraction

### Day 8-9: Transport Trait

**File**: `crates/mycelium-transport/src/lib.rs`

```rust
/// Transport mechanism for actor messages
#[async_trait]
pub trait Transport<M>: Send + Sync + Clone {
    async fn send(&self, target: ActorId, msg: M) -> Result<(), SendError>;
}

/// Local transport (Arc-based, zero-copy)
#[derive(Clone)]
pub struct LocalTransport<M> {
    handle: MailboxHandle<M>,
}

#[async_trait]
impl<M: Send + 'static> Transport<M> for LocalTransport<M> {
    async fn send(&self, _target: ActorId, msg: M) -> Result<(), SendError> {
        self.handle.send(msg, Priority::Normal).await
    }
}

/// Unix socket transport (same machine)
#[derive(Clone)]
pub struct UnixTransport {
    socket_path: PathBuf,
    // Connection pool
}

/// TCP transport (network)
#[derive(Clone)]
pub struct TcpTransport {
    addr: SocketAddr,
    // Connection pool
}
```

### Day 10-11: Adaptive Transport Selection

**File**: `crates/mycelium-config/src/deployment.rs`

```toml
# config/profiles/development.toml
[deployment]
mode = "local"  # All actors in same process

[actors]
default_mailbox_size = 100
```

```rust
/// Deployment configuration determines transport
pub enum DeploymentMode {
    /// All actors in same process (Arc-based)
    Local,

    /// Actors across processes on same machine (Unix sockets)
    MultiProcess {
        socket_dir: PathBuf,
    },

    /// Actors across machines (TCP)
    Distributed {
        nodes: Vec<NodeConfig>,
    },
}

impl ActorRef<M> {
    /// Create ref with appropriate transport based on config
    pub fn new(actor_id: ActorId, deployment: &DeploymentMode) -> Self {
        let transport = match deployment {
            DeploymentMode::Local => Transport::Local(/* ... */),
            DeploymentMode::MultiProcess { socket_dir } => {
                Transport::Unix(UnixTransport::new(socket_dir, actor_id))
            }
            DeploymentMode::Distributed { nodes } => {
                Transport::Tcp(TcpTransport::new(nodes, actor_id))
            }
        };

        Self { actor_id, transport }
    }
}
```

### Day 12-13: Supervision

**File**: `crates/mycelium-core/src/supervision.rs`

```rust
/// Supervisor manages child actors
pub struct Supervisor {
    strategy: SupervisionStrategy,
    max_restarts: usize,
    within: Duration,
    children: Vec<Child>,
}

pub enum SupervisionStrategy {
    OneForOne,
    OneForAll,
    RestForOne,
}

struct Child {
    actor_id: ActorId,
    restart_count: usize,
    last_restart: Instant,
}

impl Supervisor {
    pub async fn handle_failure(&mut self, failed_id: ActorId) {
        match self.strategy {
            SupervisionStrategy::OneForOne => {
                self.restart_one(failed_id).await;
            }
            SupervisionStrategy::OneForAll => {
                self.restart_all().await;
            }
            SupervisionStrategy::RestForOne => {
                self.restart_subsequent(failed_id).await;
            }
        }
    }
}
```

### Day 14: Documentation + Examples

**Deliverables**:
1. `examples/ping_pong.rs` - Simple actor communication
2. `examples/deployment_modes.rs` - Show local/unix/tcp
3. API documentation (rustdoc)
4. Update README with usage examples

---

## Sprint Demo (End of Week 2)

**Working Artifacts**:
1. ✅ Actor trait with lifecycle hooks
2. ✅ Location-transparent ActorRef
3. ✅ Mailbox with priority support
4. ✅ Supervision strategies
5. ✅ Local transport (Arc-based)
6. ✅ Unix socket transport (stub)
7. ✅ TCP transport (stub)
8. ✅ Configurable deployment modes

**Demo Script**:
```bash
# Show ping-pong actors (local)
cargo run --example ping_pong

# Show same code with Unix sockets
cargo run --example ping_pong --features unix

# Run test suite
cargo test

# Show test coverage
cargo tarpaulin
```

**Success Metrics**:
- All contract tests passing
- 90%+ test coverage
- Ping-pong example works in all modes
- `system.yaml` matches implementation

---

## Dependencies

```toml
# crates/mycelium-core/Cargo.toml
[dependencies]
tokio = { version = "1.35", features = ["full"] }
async-trait = "0.1"
thiserror = "1.0"
tracing = "0.1"

[dev-dependencies]
tokio-test = "0.4"
```

---

## Next Sprint Preview

**Sprint 2: Message Protocol**
- TLV codec implementation
- Zero-copy deserialization
- Message versioning
- Contract tests for all message types

This builds on actor system foundation.
