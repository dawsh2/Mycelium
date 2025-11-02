# Rust Actor Framework Comparison for Mycelium HFT System

**Date**: November 2, 2025  
**Purpose**: Evaluate existing Rust actor frameworks for high-frequency trading system requirements  
**Status**: Research Complete

---

## Executive Summary

After comprehensive research of the Rust actor ecosystem, **I recommend building a thin custom layer on top of Tokio** rather than adopting an existing framework. Here's why:

1. **No existing framework supports adaptive transport** (Arc → Unix Socket → TCP with same API)
2. **HFT requires sub-microsecond local latency** - most frameworks add 200-500ns overhead
3. **Custom implementation gives full control** over zero-copy paths and serialization
4. **Tokio provides all primitives needed** - channels, tasks, and async runtime
5. **Production HFT systems use custom solutions** for latency-critical paths

**Recommendation**: Build Mycelium's actor system directly on Tokio with:
- Custom `ActorRef<T>` abstraction supporting multiple transports
- `rkyv` for zero-copy serialization (2-4x faster than bincode)
- Supervision trees inspired by Ractor/Erlang patterns
- Bounded channels with backpressure (Tokio mpsc)

---

## Framework Analysis

### 1. Actix ⭐ Most Popular

**Maintenance Status**: Active (but diminishing for actors)
- Last release: June 2024
- Core actor framework still maintained but less emphasized
- `actix-web-actors` officially deprecated
- Focus shifted away from general-purpose actors as async/await matured

**Performance**:
- ✅ **Fastest messaging speeds** among all frameworks
- ✅ **Fastest actor spawning** 
- ✅ Tokio mpsc baseline: ~200-256ns per operation
- ⚠️ Custom runtime adds complexity

**Transport Flexibility**: ❌ **None**
- Single-process only
- No built-in distributed actor support
- No adaptive transport layer

**Supervision Model**: ✅ **Yes**
- Actor supervision and linking
- Lifecycle hooks (started, stopped, stopping)
- Restart strategies

**Type Safety**: ✅ **Strong**
- Typed messages per actor
- `Message` trait for compile-time checks
- Type-safe actor addresses

**Real-World Usage**:
- Actix Web (most popular Rust web framework)
- Used for web server concurrency, not distributed systems
- Limited HFT production examples

**HFT Suitability**: ⚠️ **Partial**
- ✅ Excellent local performance
- ✅ Low latency messaging
- ❌ No distributed support
- ❌ No adaptive transport
- ❌ Framework overhead for simple cases
- ⚠️ Diminishing ecosystem focus

---

### 2. Kameo 🆕 Newest Framework

**Maintenance Status**: ✅ **Very Active**
- Last release: March 2025
- Actively developed and maintained
- Modern design with recent Rust idioms
- Growing community

**Performance**:
- ✅ Comparable to other Tokio-based frameworks
- ✅ Competitive spawn times
- ⚠️ Slightly slower than Actix for local messaging
- Built on standard Tokio runtime

**Transport Flexibility**: ✅ **Partial**
- ✅ Built-in distributed actor support via libp2p
- ✅ Multiple protocols: TCP/IP, WebSockets, QUIC
- ❌ Not adaptive by default (network-first design)
- ❌ No Arc<T> optimization for local actors

**Supervision Model**: ✅ **Yes**
- Actor linking for supervision
- Customizable lifecycle hooks (on_start, on_stop, on_panic)
- Fault isolation (panic in one actor doesn't crash system)
- Automatic recovery mechanisms

**Type Safety**: ✅ **Strong**
- Typed message handling
- Compile-time message validation
- Request/reply pattern support

**Backpressure**: ✅ **Yes**
- Bounded and unbounded mailboxes
- Flow control for load management

**Real-World Usage**:
- Newer framework, limited production examples
- Used in distributed systems projects
- Growing adoption

**HFT Suitability**: ⚠️ **Moderate**
- ✅ Good supervision trees
- ✅ Strong type safety
- ✅ Distributed support
- ❌ libp2p overhead too heavy for HFT
- ❌ Network-first design (always serializes)
- ❌ No zero-copy local optimization
- ⚠️ Less battle-tested in production

---

### 3. Ractor 🦅 Erlang-Inspired

**Maintenance Status**: ✅ **Active**
- Last release: February 2025
- Used in production at Meta (RustConf 2024)
- Stable API, regular updates
- Strong Erlang/OTP heritage

**Performance**:
- ✅ Comparable to Kameo and Coerce
- ✅ Good spawn times
- ✅ No custom runtime overhead (pure Tokio)
- ⚠️ Serialization required for distributed actors

**Transport Flexibility**: ✅ **Partial (via ractor_cluster)**
- ✅ Distributed actors via `ractor_cluster`
- ✅ Location-transparent `ActorRef`
- ⚠️ Cluster support "relatively stable but not production-ready"
- ❌ No Arc<T> fast path for local actors
- ❌ All distributed messages must be serializable

**Supervision Model**: ✅✅ **Excellent**
- Full Erlang-style supervision trees
- `gen_server` pattern from Erlang/OTP
- Actor linking and monitoring
- Flexible supervisor hierarchies
- Each actor can supervise others (zero cost)

**Type Safety**: ✅ **Strong**
- Typed message handling
- Requires `ractor::Message` trait
- Compile-time guarantees

**Real-World Usage**: ✅ **Production-Grade**
- **Meta**: Distributed overload protection for Rust Thrift servers (RustConf 2024)
- Used for failure-protected concurrent processing pools (factories)
- System overload protection in production
- Battle-tested at scale

**HFT Suitability**: ⚠️ **Moderate**
- ✅ Excellent supervision (critical for resilience)
- ✅ Proven at Meta scale
- ✅ Clean API, no runtime overhead
- ❌ No zero-copy local messaging
- ❌ Cluster support not production-ready
- ❌ Always requires serialization for distributed

**Key Insight**: Meta uses Ractor for **distributed coordination and overload protection**, not for ultra-low-latency message passing. This suggests it's good for control plane, not data plane in HFT.

---

### 4. Coerce 🌐 Distributed-First

**Maintenance Status**: ✅ **Active**
- Last release: October 2023
- Stable, mature codebase
- Distributed-first design philosophy
- Inspired by Akka/Orleans

**Performance**:
- ⚠️ Highest spawn times (due to distributed features)
- ✅ Comparable messaging performance to Kameo/Ractor
- ⚠️ Overhead from location transparency

**Transport Flexibility**: ✅ **Location-Transparent**
- ✅ Built-in distributed actors
- ✅ Location-transparent `ActorRef` (local or remote)
- ✅ `LocalActorRef<A>` vs `RemoteActorRef<A>` abstraction
- ❌ No optimization for local-only messaging
- ❌ Always assumes potential distribution

**Supervision Model**: ✅ **Yes**
- Actor supervision capabilities
- System-level health monitoring
- Node join/leave notifications

**Type Safety**: ✅ **Strong**
- Typed actor references
- Message trait system

**Real-World Usage**:
- Used in distributed systems
- Influenced by production frameworks (Akka, Orleans)
- Limited HFT examples

**HFT Suitability**: ❌ **Poor**
- ✅ Good distributed design
- ✅ Location transparency
- ❌ Too much overhead for local messaging
- ❌ Highest spawn times
- ❌ No zero-copy optimization
- ❌ Distribution-first hurts local performance

---

### 5. Xtra 🪶 Lightweight

**Maintenance Status**: ✅ **Active**
- Last release: February 2024
- Minimal, focused design
- Multi-runtime support

**Performance**:
- ✅ Competitive spawn times
- ✅ Good messaging performance
- ✅ Runtime-agnostic (Tokio, async-std, smol, wasm)

**Transport Flexibility**: ❌ **None**
- Single-process only
- No distributed support
- No network transport

**Supervision Model**: ❌ **None**
- No built-in supervision
- No fault tolerance primitives
- Manual restart logic required

**Type Safety**: ✅ **Strong**
- Typed messages
- Type-safe addresses

**Backpressure**: ✅ **Yes**
- Bounded and unbounded mailboxes
- Ask/tell patterns

**Real-World Usage**:
- Lightweight concurrency projects
- Single-node applications
- Limited production examples

**HFT Suitability**: ⚠️ **Limited**
- ✅ Low overhead
- ✅ Simple, clean API
- ❌ No supervision (critical for HFT resilience)
- ❌ No distributed support
- ⚠️ Would need custom supervision layer

---

### 6. Bastion 🏰 Fault-Tolerant Runtime

**Maintenance Status**: ❌ **Abandoned/Dormant**
- Last release: July 2020 (v0.4)
- Actively seeking maintainers (GitHub issue)
- Last significant activity: 2022
- Unmaintained dependencies

**Performance**:
- Historic claims of high performance
- Lightweight process model
- No recent benchmarks

**Transport Flexibility**: ⚠️ **Unknown**
- Designed for distributed systems
- Documentation outdated
- Unclear current capabilities

**Supervision Model**: ✅ **Yes (in theory)**
- Erlang-inspired supervision
- Fault tolerance primitives
- Dynamic dispatch model

**HFT Suitability**: ❌ **Not Recommended**
- ❌ Project effectively abandoned
- ❌ Security/maintenance risks
- ❌ Outdated dependencies
- ❌ No recent production usage

**Verdict**: Do not use. Consider Ractor instead for Erlang-style patterns.

---

### 7. Riker 🎭 Actor Framework

**Maintenance Status**: ⚠️ **Low/Dormant**
- Last significant update: November 2020
- Seeking contributors
- Open security issues (unmaintained deps)
- Pre-1.0 status

**HFT Suitability**: ❌ **Not Recommended**
- ⚠️ Minimal maintenance
- ⚠️ May not work with modern Rust
- ❌ Not suitable for production HFT

**Verdict**: Consider Ractor or Kameo as modern alternatives.

---

### 8. Axiom 🔮 Cluster Framework

**Maintenance Status**: ⚠️ **Unclear**
- Focuses on cluster management
- TCP-based distributed actors
- Limited recent activity
- Fork exists (Maxim) suggesting concerns

**Transport Flexibility**: ✅ **Partial**
- TCP cluster support
- Location-agnostic actors
- Immutable message passing

**HFT Suitability**: ⚠️ **Unclear**
- Limited benchmarks
- Unknown production usage
- Unclear maintenance commitment

**Verdict**: Insufficient data. Consider Ractor or Kameo for proven alternatives.

---

### 9. Tokio Native Patterns 🚀 DIY Approach

**Maintenance Status**: ✅✅ **Official Tokio Support**
- Part of Tokio project
- Well-documented patterns
- Alice Ryhl's canonical guide
- Used in mini-redis tutorial

**Performance**: ✅✅ **Excellent**
- Tokio mpsc baseline: ~200-256ns per operation
- No framework overhead
- Direct channel access
- Optimal for local messaging

**Transport Flexibility**: ⚠️ **DIY**
- Build exactly what you need
- Full control over transport layer
- Can implement Arc<T> zero-copy
- Can add Unix socket/TCP as needed

**Supervision Model**: ⚠️ **DIY**
- Implement your own supervision
- JoinHandle-based monitoring
- Custom restart logic
- Can follow Erlang patterns

**Type Safety**: ✅ **Strong**
- Enum-based message types
- Trait-based dispatch
- oneshot channels for replies

**Key Patterns**:
```rust
// Actor split into Task + Handle
struct MyActor { rx: mpsc::Receiver<Message> }
struct MyActorHandle { tx: mpsc::Sender<Message> }

// Spawn pattern
fn spawn_actor() -> MyActorHandle {
    let (tx, rx) = mpsc::channel(32);
    tokio::spawn(async move {
        let actor = MyActor { rx };
        actor.run().await;
    });
    MyActorHandle { tx }
}
```

**Production Lessons** (from community):
1. ✅ Use bounded channels for backpressure
2. ⚠️ Avoid cycles with bounded channels (deadlock risk)
3. ✅ Use unbounded or `try_send` to break cycles
4. ⚠️ Don't block the event loop (use rayon for CPU work)
5. ✅ Implement graceful shutdown carefully
6. ⚠️ Unbounded mailboxes can fill and crash system

**HFT Suitability**: ✅✅ **Excellent**
- ✅ Lowest possible overhead
- ✅ Full control over hot paths
- ✅ Can optimize Arc<T> for local
- ✅ Can add serialization for remote
- ✅ Production-proven pattern
- ✅ No framework lock-in
- ⚠️ Requires building supervision yourself

---

## Performance Comparison Summary

### Local Message Passing Latency

| Framework | Latency | Notes |
|-----------|---------|-------|
| Tokio mpsc (baseline) | 200-256ns | Raw channel, no actor framework |
| Actix | ~200-300ns | Fastest framework, optimized runtime |
| Kameo | ~250-350ns | Tokio-based, comparable to Ractor |
| Ractor | ~250-350ns | Pure Tokio, no custom runtime |
| Coerce | ~300-400ns | Higher due to distributed features |
| Xtra | ~250-350ns | Varies by runtime choice |

**HFT Context**: Sub-millisecond = 1,000,000ns. All frameworks meet this, but:
- **Inter-thread communication**: ~100ns (not negligible in HFT)
- **50-120ns targets** achieved with custom implementations
- **Framework overhead**: 50-200ns added on top of channels

### Actor Spawn Time

| Framework | Relative Speed |
|-----------|----------------|
| Actix | Fastest |
| Kameo | Fast |
| Ractor | Fast |
| Xtra | Fast |
| Coerce | Slowest (distributed overhead) |

---

## Zero-Copy Serialization: rkyv vs serde/bincode

### Performance Comparison

| Metric | rkyv | bincode/serde | Improvement |
|--------|------|---------------|-------------|
| Deserialization | Zero-copy (direct access) | Must reconstruct objects | 2-4x faster |
| Read throughput | 4.0 GB/s | 2.1 GB/s | 1.9x |
| Write throughput | ~40% better | baseline | 1.4x |
| Access/Update | 100% (best) | Slower | Significantly faster |

**Why rkyv for HFT**:
- ✅ True zero-copy (no object reconstruction)
- ✅ 2-4x faster than bincode even with validation
- ✅ Direct memory access to archived data
- ✅ Validated mode available for safety
- ✅ Perfect for TLV protocol

**Trade-offs**:
- ⚠️ More complex API than serde
- ⚠️ Requires careful memory alignment
- ✅ Worth it for HFT latency requirements

---

## Mycelium Requirements Analysis

### Requirement 1: Low Latency (Sub-millisecond) ✅

**Assessment**: All modern frameworks meet sub-millisecond requirement.

**However**:
- Sub-millisecond = 1,000,000ns
- Actix: ~250ns local messaging
- Custom Tokio: ~200ns baseline
- **HFT often targets 50-120ns** for critical paths

**Recommendation**: Custom Tokio actors for hot path, framework for cold path.

---

### Requirement 2: Adaptive Transport ❌

**Assessment**: No existing framework supports this natively.

| Framework | Arc<T> Local | Unix Socket | TCP | Same API |
|-----------|--------------|-------------|-----|----------|
| Actix | Implicit | ❌ | ❌ | N/A |
| Kameo | ❌ | ✅ (libp2p) | ✅ (libp2p) | ⚠️ |
| Ractor | ❌ | ⚠️ (cluster) | ✅ (cluster) | ⚠️ |
| Coerce | ⚠️ | ⚠️ | ✅ | ✅ |
| Custom | ✅ | ✅ | ✅ | ✅ |

**Key Issues**:
1. **Coerce/Kameo/Ractor**: Always serialize, even for local actors
2. **No framework optimizes Arc<T> fast path**
3. **Location transparency != adaptive transport**

**Recommendation**: Must build custom transport layer.

---

### Requirement 3: Supervision Trees ✅

**Assessment**: Several frameworks provide this.

| Framework | Supervision | Quality |
|-----------|-------------|---------|
| Ractor | ✅✅ | Erlang-style, excellent |
| Kameo | ✅ | Good, lifecycle hooks |
| Actix | ✅ | Good, mature |
| Coerce | ✅ | Good, distributed-aware |
| Xtra | ❌ | None |
| Custom | ⚠️ | DIY |

**Recommendation**: Study Ractor's patterns, implement custom supervision matching your needs.

---

### Requirement 4: Typed Messages ✅

**Assessment**: All frameworks provide strong typing.

- ✅ All use Rust's type system
- ✅ Compile-time message validation
- ✅ Type-safe actor references
- ✅ Enum or trait-based dispatch

**Recommendation**: Any approach works; Tokio native enum pattern is simplest.

---

### Requirement 5: Zero-Copy Serialization ⚠️

**Assessment**: No framework integrates rkyv natively.

**Options**:
1. **Use rkyv manually** in message handlers
2. **Build custom message trait** supporting zero-copy
3. **Hybrid**: Arc<T> for local, rkyv for remote

**Recommendation**: Build custom `Message` trait supporting:
- Local: `Arc<T>` (no serialization)
- Remote: `rkyv::Archived<T>` (zero-copy)

---

### Requirement 6: Backpressure ✅

**Assessment**: Most frameworks provide bounded mailboxes.

| Framework | Bounded | Unbounded | Backpressure |
|-----------|---------|-----------|--------------|
| Actix | ✅ | ❌ | Yes |
| Kameo | ✅ | ✅ | Yes |
| Ractor | ❌ | ✅ | Limited |
| Coerce | ❌ | ✅ | Limited |
| Xtra | ✅ | ✅ | Yes |
| Tokio mpsc | ✅ | ✅ | Yes |

**Recommendation**: Tokio mpsc provides excellent bounded channels with backpressure.

---

### Requirement 7: Actor Discovery ⚠️

**Assessment**: Limited built-in support.

| Framework | Registry | Distributed Discovery |
|-----------|----------|----------------------|
| Ractor | ✅ Named registry | ⚠️ (cluster) |
| Kameo | ⚠️ | ✅ (libp2p) |
| Coerce | ✅ | ✅ |
| Actix | ✅ System registry | ❌ |
| Custom | Build it | Build it |

**Recommendation**: Build custom registry:
- Local: `HashMap<ActorId, ActorRef>`
- Remote: Discovery protocol (etcd, consul, or custom)

---

### Requirement 8: Production-Grade ⚠️

**Assessment**: Mixed results.

**Production-Proven**:
- ✅ **Actix**: Massive web framework usage (but not distributed)
- ✅ **Ractor**: Meta production (Thrift overload protection)
- ✅ **Tokio patterns**: Widely used (mini-redis, etc.)
- ⚠️ **Kameo**: Newer, growing adoption
- ⚠️ **Coerce**: Some production use, less documented

**HFT-Specific Production**:
- ❌ No framework has documented HFT production use
- ✅ Rust HFT projects use custom actor implementations
- ✅ Custom Tokio patterns most common in low-latency systems

**Why?**
1. HFT requires custom optimization
2. Zero-copy paths critical
3. Adaptive transport unique to Mycelium
4. Framework overhead unacceptable for hot path

**Recommendation**: Follow HFT industry pattern - custom implementation.

---

## Real-World HFT Insights

### Erlang/Actor Model in Finance

**Pros**:
- ✅ Fault tolerance critical for trading systems
- ✅ Concurrent processing ideal for market data
- ✅ Easy to scale across cores
- ✅ Goldman Sachs uses Erlang (microsecond latency)
- ✅ Delta Exchange uses Erlang for HFT

**Cons**:
- ❌ Erlang 12-16x slower than C++ for compute
- ❌ Microsecond-level logic suffers in Erlang
- ⚠️ Network/serialization overhead kills latency

**Lesson**: Actor model good for **coordination and resilience**, not for **ultra-low-latency hot path**.

### Rust HFT Best Practices (2024-2025)

1. ✅ **Zero-allocation hot paths** - critical
2. ✅ **Lock-free data structures** where possible
3. ✅ **Rust 2-4x better latency consistency** than GC languages
4. ⚠️ **Inter-thread communication (~100ns)** is not negligible
5. ✅ **Sub-microsecond requires custom solutions**
6. ✅ **Nanosecond-level performance achievable** (50-120ns targets)

### Production HFT Pattern

```
Control Plane (Actor Framework)
    ├── Strategy coordination
    ├── Risk management
    ├── Configuration
    └── Monitoring

Data Plane (Custom, Zero-Copy)
    ├── Market data ingestion
    ├── Order execution
    ├── Arbitrage detection
    └── Critical trading logic
```

**Recommendation**: Mycelium should follow this split architecture.

---

## Framework Recommendations by Use Case

### If You Need...

#### 1. **Single-Process, Local Actors Only**
**Recommendation**: Actix or Tokio Native
- Actix: If you want mature supervision
- Tokio Native: If you want zero overhead

#### 2. **Distributed Actors, Standard Latency**
**Recommendation**: Kameo or Ractor
- Kameo: Modern, active, good docs
- Ractor: Erlang patterns, Meta-proven

#### 3. **Distributed Actors, Location Transparency**
**Recommendation**: Coerce
- Good abstraction over local/remote
- Higher overhead acceptable

#### 4. **HFT, Sub-Microsecond Latency**
**Recommendation**: Custom Tokio + rkyv
- No framework meets requirements
- Build thin layer on Tokio
- Full control over hot path

---

## Recommendation for Mycelium

### Primary Recommendation: Custom Tokio-Based Implementation

**Rationale**:
1. ✅ **Adaptive transport is unique** - no framework supports this
2. ✅ **HFT latency targets** require zero overhead
3. ✅ **Full control** over serialization strategy
4. ✅ **Production HFT pattern** is custom implementations
5. ✅ **Tokio provides all primitives** needed
6. ✅ **No framework lock-in** or overhead

### Architecture Design

```rust
// Core abstraction
pub enum ActorRef<T> {
    Local(Arc<LocalRef<T>>),      // Arc<T> messages, zero-copy
    UnixSocket(UnixRef<T>),        // Same machine, serialized once
    Tcp(TcpRef<T>),                // Network, full serialization
}

// Unified send API
impl<T> ActorRef<T> {
    pub async fn send(&self, msg: T) -> Result<()> {
        match self {
            Local(r) => r.send(Arc::new(msg)).await,
            UnixSocket(r) => r.send_serialized(msg).await,
            Tcp(r) => r.send_serialized(msg).await,
        }
    }
}

// Message trait supporting both paths
pub trait Message: Send + Sync + 'static {
    // For local messaging
    fn as_arc(&self) -> Arc<dyn Any + Send + Sync>;
    
    // For remote messaging (rkyv)
    fn serialize_rkyv(&self) -> Result<Vec<u8>>;
    fn deserialize_rkyv(bytes: &[u8]) -> Result<Self>;
}
```

### Implementation Layers

#### Layer 1: Core Actor System (mycelium-core)
- Actor trait and lifecycle
- Supervision trees (inspired by Ractor)
- Actor registry (local)
- Tokio-based task spawning

#### Layer 2: Transport Layer (mycelium-transport)
- `ActorRef<T>` enum (Local/Unix/Tcp)
- Transport negotiation
- Connection management
- Serialization abstraction

#### Layer 3: Protocol Layer (mycelium-protocol)
- rkyv-based message encoding
- TLV protocol integration
- Zero-copy deserialization
- Schema versioning

#### Layer 4: Runtime Layer (mycelium-runtime)
- Actor placement (NUMA-aware)
- Distributed discovery
- Health monitoring
- Metrics collection

### Key Design Decisions

#### 1. Message Passing Strategy

**Local (same process)**:
```rust
// Zero-copy using Arc
let msg = Arc::new(MarketData { ... });
actor_ref.send(msg).await?;  // No serialization
```

**Unix Socket (same machine)**:
```rust
// Serialize once with rkyv, zero-copy deserialize
let bytes = rkyv::to_bytes(&msg)?;
unix_ref.send(bytes).await?;  // Single copy to socket
// Receiver: zero-copy access via rkyv::Archived<T>
```

**TCP (remote machine)**:
```rust
// Full serialization with rkyv
let bytes = rkyv::to_bytes(&msg)?;
tcp_ref.send(bytes).await?;  // Network transmission
```

#### 2. Supervision Strategy

**Borrow from Ractor**:
- Supervisor actors with restart strategies
- Actor linking (bidirectional monitoring)
- Hierarchical supervision trees
- Failure isolation

**Custom Additions**:
- Circuit breakers for trading strategies
- Graceful degradation (drop non-critical actors)
- Fast failover for critical actors
- State persistence hooks

#### 3. Type Safety

**Use Rust's type system fully**:
```rust
// Each actor has typed message enum
enum MarketDataMsg {
    NewTick(Arc<Tick>),
    Subscribe(ActorRef<MarketDataMsg>),
    Unsubscribe(ActorId),
}

// Enforce at compile time
impl Actor for MarketDataActor {
    type Message = MarketDataMsg;
    
    async fn handle(&mut self, msg: Self::Message) {
        match msg {
            NewTick(tick) => self.process_tick(tick).await,
            Subscribe(actor) => self.subscribers.push(actor),
            Unsubscribe(id) => self.subscribers.retain(|a| a.id != id),
        }
    }
}
```

#### 4. Backpressure

**Use Tokio mpsc bounded channels**:
```rust
// Bounded mailbox (32 messages)
let (tx, rx) = mpsc::channel(32);

// Send blocks when full (backpressure)
tx.send(msg).await?;

// Or fail fast
tx.try_send(msg)?;
```

**Configuration**:
- Hot path actors: Bounded channels (16-32)
- Cold path actors: Unbounded for convenience
- Critical actors: try_send + circuit breaker

#### 5. Performance Optimization

**Hot Path (< 1% of code, 99% of time)**:
- Direct Tokio channels
- Arc<T> message passing
- No serialization
- Single-threaded actors where possible
- Lock-free queues

**Cold Path (99% of code, < 1% of time)**:
- Full actor abstraction
- Supervision trees
- Network-aware messaging
- Standard patterns

### Benefits of Custom Approach

1. ✅ **Adaptive transport** - core requirement met
2. ✅ **Zero overhead** for local hot path
3. ✅ **Zero-copy serialization** with rkyv
4. ✅ **Full control** over performance characteristics
5. ✅ **No framework limitations** or opinions
6. ✅ **Learn from best practices** (Ractor, Tokio, Erlang)
7. ✅ **Production-grade patterns** proven in HFT
8. ✅ **Exactly what you need** - no more, no less

### Drawbacks

1. ⚠️ **More initial development** time
2. ⚠️ **Maintenance burden** on your team
3. ⚠️ **Must implement supervision** yourself
4. ⚠️ **Testing complexity** for distributed scenarios
5. ⚠️ **Documentation** needed for team

### Mitigation

1. ✅ **Start simple** - Tokio actors + supervision
2. ✅ **Add transport gradually** - Local → Unix → TCP
3. ✅ **Borrow patterns** from Ractor/Alice Ryhl
4. ✅ **Comprehensive tests** for each transport
5. ✅ **Document as you build** - keep it current

---

## Alternative Recommendation: Hybrid Approach

If fully custom is too much, consider:

### Option A: Ractor + Custom Transport

**Use Ractor for**:
- Actor lifecycle
- Supervision trees
- Local messaging
- Registry

**Build custom**:
- Transport layer (Arc/Unix/TCP)
- Serialization strategy (rkyv)
- NUMA placement
- Performance-critical paths

**Pros**:
- ✅ Proven supervision model
- ✅ Less code to write
- ✅ Meta-validated patterns
- ⚠️ Still need custom transport

**Cons**:
- ⚠️ Ractor cluster not production-ready
- ⚠️ Framework overhead for hot path
- ⚠️ Limited control over internals

### Option B: Tokio + Kameo Supervision

**Use Tokio patterns for**:
- Hot path actors
- Critical latency paths
- Arc<T> messaging

**Use Kameo for**:
- Cold path actors
- Supervision trees
- Non-critical paths

**Pros**:
- ✅ Best of both worlds
- ✅ Optimize where needed
- ✅ Framework where convenient

**Cons**:
- ⚠️ Two systems to maintain
- ⚠️ Complexity in hybrid design
- ⚠️ Team must understand both

---

## Implementation Roadmap

### Phase 1: Foundation (Week 1-2)
- [ ] Basic Tokio actor pattern (task + handle)
- [ ] Simple supervision (restart on panic)
- [ ] Local registry (HashMap)
- [ ] Bounded mailboxes with backpressure
- [ ] Basic message enum pattern

### Phase 2: Local Optimization (Week 3-4)
- [ ] `ActorRef::Local` with Arc<T>
- [ ] Zero-copy local messaging
- [ ] Supervision trees (Erlang-style)
- [ ] Actor lifecycle hooks
- [ ] Performance benchmarks vs Actix

### Phase 3: Serialization (Week 5-6)
- [ ] rkyv integration
- [ ] `Message` trait with serialize/deserialize
- [ ] Zero-copy deserialization testing
- [ ] TLV protocol integration
- [ ] Benchmarks vs bincode

### Phase 4: Unix Sockets (Week 7-8)
- [ ] `ActorRef::UnixSocket` implementation
- [ ] Same-machine messaging
- [ ] Serialization + zero-copy deserialize
- [ ] Multi-process coordination
- [ ] Performance testing

### Phase 5: TCP/Distributed (Week 9-12)
- [ ] `ActorRef::Tcp` implementation
- [ ] Connection management
- [ ] Discovery protocol (etcd/consul)
- [ ] Distributed supervision
- [ ] End-to-end testing

### Phase 6: Production Hardening (Week 13-16)
- [ ] Circuit breakers
- [ ] Graceful degradation
- [ ] Comprehensive error handling
- [ ] Monitoring/observability hooks
- [ ] Load testing at scale
- [ ] Documentation

---

## Key Lessons from Research

### 1. No Silver Bullet
No existing framework meets all Mycelium requirements. Adaptive transport is unique.

### 2. HFT Requires Custom
Production HFT systems use custom actor implementations for latency-critical paths.

### 3. Tokio Is Sufficient
Tokio provides all necessary primitives. Frameworks add convenience, not capability.

### 4. Supervision Matters
Erlang-style supervision critical for resilience. Study Ractor's approach.

### 5. Zero-Copy Is Critical
rkyv provides 2-4x performance improvement over bincode for deserialization.

### 6. Separate Hot/Cold Path
Actor model excellent for coordination (cold path), custom optimization for hot path.

### 7. Location Transparency ≠ Adaptive
Coerce/Kameo provide location transparency but always serialize. Not what we need.

### 8. Meta Uses Ractor Wisely
Ractor used for control plane (overload protection), not data plane. Follow this pattern.

### 9. Community Patterns Work
Tokio actor pattern (task + handle) is battle-tested and production-proven.

### 10. Start Simple, Optimize Later
Build core abstractions first, optimize hot paths based on profiling.

---

## Conclusion

**For Mycelium HFT system, I recommend building a custom actor layer on Tokio because**:

1. ✅ **Adaptive transport is unique** - no framework supports this core requirement
2. ✅ **HFT latency demands** full control over hot paths
3. ✅ **Tokio provides sufficient primitives** for actor model
4. ✅ **Production HFT pattern** is custom implementations
5. ✅ **Zero-copy optimization** requires custom message handling
6. ✅ **Learn from best frameworks** (Ractor supervision, Alice Ryhl patterns)
7. ✅ **No framework lock-in** - full flexibility
8. ✅ **Exactly what you need** - optimize where it matters

**Implementation strategy**:
- Start with Tokio actor patterns (proven, simple)
- Add supervision trees (borrow from Ractor)
- Build transport layer (Arc → Unix → TCP)
- Integrate rkyv (zero-copy serialization)
- Optimize hot path (profile-guided)
- Harden for production (monitoring, testing)

**Timeline**: 16 weeks for full implementation, 4 weeks for MVP (local actors + supervision).

**Risk mitigation**: Build incrementally, test each layer, benchmark continuously.

This approach gives Mycelium the **adaptive transport**, **sub-microsecond local latency**, and **production-grade resilience** needed for high-frequency trading, while maintaining the flexibility to optimize and evolve the system as requirements become clearer.

---

## References

1. **Comparing Rust Actor Libraries** - https://tqwewe.com/blog/comparing-rust-actor-libraries/
2. **Actors with Tokio (Alice Ryhl)** - https://ryhl.io/blog/actors-with-tokio/
3. **Ractor Documentation** - https://slawlor.github.io/ractor/
4. **rkyv Performance** - https://david.kolo.ski/blog/rkyv-is-faster-than/
5. **Rust Serialization Benchmarks** - https://github.com/djkoloski/rust_serialization_benchmark
6. **Meta RustConf 2024** - Ractor for distributed overload protection
7. **Tokio Tutorial (Channels)** - https://tokio.rs/tokio/tutorial/channels
8. **Tiny Tokio Actors** - https://fdeantoni.medium.com/tiny-tokio-actors-3a2ec958ef43

---

**Next Steps**:
1. Review this analysis with the team
2. Prototype basic Tokio actor pattern (2-3 days)
3. Benchmark against Actix for validation
4. Start Phase 1 implementation if approved
5. Document architecture decisions in `/docs/ARCHITECTURE.md`
