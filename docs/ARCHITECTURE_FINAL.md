# Final Architecture: Mycelium Library + Bandit Framework

**Date:** 2025-11-03  
**Status:** Approved and Documented  
**Next Phase:** Implementation of typed actor discovery

---

## Executive Summary

**Mycelium** = Reusable messaging library providing transport-agnostic primitives  
**Bandit** = Trading framework built on Mycelium (example application, separate repo)

This architecture achieves clean separation of concerns: Mycelium provides runtime primitives (actors, pub/sub, transports), while Bandit provides orchestration (service registry, topology, deployment).

---

## Core Principle

> **Library provides primitives. Framework provides orchestration.**

**What belongs in Mycelium (library):**
- ✅ TLV codegen from `contracts.yaml`
- ✅ Transport abstraction (Arc/Unix/TCP)
- ✅ Actor runtime and typed references
- ✅ MessageBus (pub/sub + mailboxes)
- ✅ ServiceContext API
- ✅ Observability hooks

**What belongs in Bandit (framework):**
- ✅ Service implementations (PolygonAdapter, etc.)
- ✅ Service registry (type-safe spawning)
- ✅ Topology configuration (deployment)
- ✅ Config loading (business logic)
- ✅ Node agent (multi-node support)

---

## Current Implementation Status

### ✅ Phase 1: Core Messaging (COMPLETE)
- TLV protocol with zerocopy serialization
- MessageBus with pub/sub
- Local transport (Arc) - 47ns baseline
- Performance: 8M messages/sec per core

### ✅ Phase 2: Actor System (COMPLETE)
- Actor trait with lifecycle hooks
- ActorRuntime for spawning
- ActorRef<M> for typed references
- Mailbox = topic pattern
- Basic supervision

### ⏳ Phase 3: Actor-Centric API (IN PROGRESS)
**Target:** Type-safe actor discovery + unified ServiceContext

**What's being added:**
1. **Typed actor discovery** - `ctx.get_actor::<RiskManager>()`
2. **ServiceContext** - Unified API for pub/sub + actors
3. **Service macro** - Wraps services as actors
4. **Examples** - Demonstrates patterns

**Implementation plan:** See `docs/STATUS.md`  
**Design details:** See `docs/implementation/TYPED_ACTOR_DISCOVERY.md`

---

## API Overview

### For Service Authors (Using Mycelium)

```rust
// 1. Define your service
#[mycelium::service]
impl PolygonAdapter {
    async fn run(&mut self, ctx: ServiceContext) -> Result<()> {
        // Pub/sub broadcast (1→N)
        ctx.emit(V2Swap { ... }).await?;
        
        // Actor unicast (1→1)
        let risk_mgr = ctx.get_actor::<RiskManager>()?;
        risk_mgr.send(CheckPosition { ... }).await?;
        
        Ok(())
    }
}
```

**Key point:** Services never know about transports. Same code works with Arc/Unix/TCP.

---

### For Framework Authors (Using Mycelium + Building Orchestration)

```rust
// 2. Create service registry (in Bandit, not Mycelium)
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
// Expands to type-safe spawning code

// 3. Deploy from topology
#[tokio::main]
async fn main() -> Result<()> {
    let node = env::var("NODE")?;  // "adapters", "strategies", etc.
    
    BanditDeployment::from_topology("topology.yaml")?
        .run(&node)  // Spawns services for this node
        .await
}
```

**Key point:** Topology determines transport at deployment time, not compile time.

---

### Simple Node Configuration (Bandit)

```toml
# config/adapters.toml
transport = "tcp://10.0.1.10:9000"
services = ["PolygonAdapter", "BaseAdapter"]

# config/strategies.toml
transport = "local"  # Arc - shared memory
services = ["ArbitrageService", "RiskManager"]

# config/execution.toml
transport = "unix:///tmp/bandit.sock"
services = ["ExecutionService"]
```

**Same binary, different NODE env var → different transport automatically.**

**Note:** No complex YAML topology. Simple TOML configs. Service-specific configs live in service crates.

---

## Key Design Decisions

### 1. Actor-Centric Design

**Every service is an actor:**
- Typed references: `ActorRef<RiskManager>`
- Service discovery: `ctx.get_actor::<RiskManager>()`
- Direct messaging: `risk_mgr.send(msg)`
- Pub/sub coexists: `ctx.emit(msg)`

**Benefits:**
- Fault isolation (one actor crash doesn't kill others)
- Per-actor supervision (restart policies)
- State isolation (no Arc<Mutex<T>> needed)
- Type-safe routing

---

### 2. Transport Abstraction via Topology

**Services never know about transport:**

```rust
// Same code everywhere
ctx.emit(swap).await?;
ctx.get_actor::<RiskManager>()?.send(msg).await?;
```

**Topology determines transport:**
- Same node → Arc (shared memory, 47ns)
- Same host → Unix sockets (~5μs)
- Different hosts → TCP (~500μs)

---

### 3. Macro-Generated Registry (Type Safety Bridge)

**Problem:** Topology YAML has strings (unavoidable), but we want type safety.

**Solution:** Macro validates at compile time:

```rust
mycelium::service_registry! {
    PolygonAdapter => PolygonAdapter::new(config),
    //               ^^^^^^^^^^^^^^^ Compile error if constructor doesn't exist
}
```

**Result:**
- Strings only in YAML (unavoidable) and macro (validated)
- Runtime validation (fail fast on unknown services)
- Type-checked constructors

---

### 4. Singleton Actor Constraint

**Design choice:** `get_actor::<T>()` assumes one instance per type.

**Rationale:**
- Most services are singletons (RiskManager, ExecutionService)
- Simple implementation (no instance keys)
- Clear semantics

**Multi-instance workaround:**
```rust
// Option A: Wrapper types
struct PolygonMainnet(PolygonAdapter);
struct PolygonTestnet(PolygonAdapter);
runtime.spawn(PolygonMainnet(adapter)).await;

// Option B: Fallback to ActorId
let binance_id = runtime.spawn(BinanceAdapter::new()).await.actor_id();
ctx.send_to(binance_id, msg).await;
```

---

## Implementation Roadmap

**Phase 3 (Current):** Actor-centric API - 1-2 weeks
- Typed actor discovery
- ServiceContext unification
- Service macro enhancement
- Examples and docs

**Phase 4 (Next):** Service registry - 1 week
- `service_registry!` macro
- Topology parser
- BanditDeployment orchestrator

**Phase 5 (Future):** Multi-transport - 2 weeks
- TLV wire format
- Unix socket transport
- TCP transport
- Cross-process routing

**Phase 6 (Optional):** Node agent - 1-2 weeks
- Multi-node deployment
- Process supervision
- Health monitoring

**See `docs/STATUS.md` for detailed breakdown.**

---

## Benefits of This Design

### 1. Transport Abstraction Preserved
Services are deployment-agnostic. Same code works in dev (single-process) and prod (distributed).

### 2. Type Safety
- `ActorRef<T>` prevents sending wrong message types
- Macro validates service constructors at compile time
- Runtime validation fails fast on configuration errors

### 3. Clean Separation
- Mycelium is reusable (games, analytics, trading)
- Bandit is trading-specific (uses Mycelium APIs)
- Clear boundary: library vs. framework

### 4. Flexibility
- Single-process: All Arc transport, 47ns latency
- Multi-process: Unix sockets, sub-10μs latency
- Distributed: TCP, sub-millisecond latency

### 5. Performance
- Zero-copy shared memory when co-located
- No serialization overhead for local transport
- Pluggable transports (RDMA/DPDK possible)

### 6. Extensibility
- Add new transports without changing service code
- Add new observability hooks (metrics, tracing)
- Pluggable supervision strategies

### 7. Simplicity
- Minimal API surface in library
- Services use high-level abstractions
- Framework handles deployment complexity

---

## Documentation Map

### Architecture and Rationale
- **`docs/ARCHITECTURE_FINAL.md`** (this doc) - High-level overview
- **`docs/DESIGN_DECISIONS.md`** - Design rationale and trade-offs
- **`docs/STATUS.md`** - Implementation status and roadmap

### Implementation Guides
- **`docs/ACTORS.md`** - Actor system design
- **`docs/TRANSPORT.md`** - Transport architecture
- **`docs/implementation/TYPED_ACTOR_DISCOVERY.md`** - Phase 3.1 design
- **`docs/implementation/SERVICE-API-DESIGN.md`** - Service API spec

### Reference
- **`README.md`** - Quick start and examples
- **`crates/*/README.md`** - Per-crate documentation

---

## Examples

### Basic Service with Pub/Sub
```bash
cargo run --example simple_pubsub
```

Shows publisher/subscriber pattern with local transport.

### Actor System
```bash
cargo run --example actor_demo
```

Shows actor spawning, messaging, and lifecycle.

### Service with Actors (Coming in Phase 3)
```bash
cargo run --example service_with_actors
```

Will demonstrate `get_actor<T>()` and ServiceContext.

---

## FAQ

### Q: Why separate Mycelium and Bandit?

**A:** Reusability. Mycelium should be useful for games, analytics, etc. - not just trading. Bandit shows how to build a framework on top.

---

### Q: Why actors instead of just pub/sub?

**A:** Fault isolation. In multi-entity systems (20+ exchange connections, 100+ strategies), one failure shouldn't crash everything. Actors provide 10-50ms recovery vs. 5-10s process restart.

---

### Q: Why not fully monomorphic channels?

**A:** Binary size and flexibility. The 1-5ns downcast overhead is negligible compared to 200ns baseline latency. Envelope provides metadata (type_id, sequence, routing) that's essential for remote transports.

---

### Q: How do I handle multiple instances of the same actor type?

**A:** Two options:
1. Wrapper types: `PolygonMainnet(PolygonAdapter)`, `PolygonTestnet(PolygonAdapter)`
2. Fallback to ActorId: Track IDs manually for multi-instance

Most services are singletons, so this is rarely needed.

---

### Q: Where should the `service_registry!` macro live?

**A:** In Mycelium (`mycelium-service-macro` crate). The macro itself is generic - only the service implementations are domain-specific. This way, any framework (Bandit, game server, etc.) can use it.

---

### Q: Can I use Mycelium without actors?

**A:** Yes! Actors are optional. You can use pure pub/sub if that fits your use case. Actors are for stateful, long-lived services with supervision needs.

---

### Q: What's the performance overhead of ServiceContext vs. raw MessageBus?

**A:** ~120ns for `ctx.emit()` vs. ~47ns for raw `publish()`. The 2.5x overhead includes:
- Topic routing (40ns)
- Timing/metrics (40ns)
- Trace logging (1ns when disabled)

Worth it for full observability.

---

## Next Steps

1. **Review this architecture** - Team alignment on design
2. **Begin Phase 3** - Implement typed actor discovery
   - See `docs/implementation/TYPED_ACTOR_DISCOVERY.md` for plan
   - Estimated: 1-2 days coding + 1 day testing/docs
3. **Set up Bandit repo** - Prepare for Phase 4 (service registry)
4. **Write migration guide** - Document API changes from Phase 2 → Phase 3

---

## Success Metrics

**Phase 3 complete when:**
- ✅ Can write `ctx.get_actor::<T>()` instead of tracking ActorIds
- ✅ ServiceContext unifies pub/sub and actor APIs
- ✅ Services are deployment-agnostic (same code, any transport)
- ✅ Examples demonstrate patterns
- ✅ All tests pass

**Overall project success:**
- ✅ Single binary works with Arc/Unix/TCP transports
- ✅ Type-safe service registry from topology
- ✅ Performance: <100ns local, <10μs Unix, <1ms TCP
- ✅ Documentation covers all use cases
- ✅ Bandit framework demonstrates real-world usage

---

## Appendix: Design History

**Phase 1 (Dec 2024):** Core messaging with TLV protocol  
**Phase 2 (Jan 2025):** Actor system with supervision  
**Phase 3 (Nov 2025 - current):** Actor-centric service API  
**Phase 4 (planned):** Service registry and topology  
**Phase 5 (planned):** Multi-transport support  
**Phase 6 (future):** Node agent for deployment

**Key insight from external analysis:** Separate library primitives (Mycelium) from orchestration tooling (Bandit). This unlocked clean reusability.

---

**Approved by:** @daws  
**Review date:** 2025-11-03  
**Status:** Ready for Phase 3 implementation
