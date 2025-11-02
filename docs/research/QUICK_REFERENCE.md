# Actor Framework Research - Quick Reference

**One-page summary for quick decisions**

---

## The Verdict

**Build custom Tokio-based actor system.**

No existing framework supports adaptive transport (Arc<T> → Unix → TCP with same API).

---

## Framework Comparison (30-Second Version)

| Framework | Good For | Bad For | Status |
|-----------|----------|---------|--------|
| **Actix** | Single-process performance | Distribution | Active but diminishing |
| **Kameo** | Modern distributed systems | HFT latency | Very active |
| **Ractor** | Erlang supervision trees | Production clustering | Active (Meta uses) |
| **Coerce** | Location transparency | Local optimization | Active |
| **Xtra** | Simple local concurrency | Production features | Active |
| **Tokio DIY** | **Full control, HFT** | **Quick prototypes** | **Official runtime** |

**Winner**: Tokio DIY (custom implementation)

---

## Why Custom?

**One reason**: Adaptive transport
```rust
// Same API, zero serialization if local, serialized if remote
let actor: ActorRef<Msg> = ...;  // Local, Unix, or TCP
actor.send(msg).await?;           // Arc<T> if local, rkyv if remote
```

**No framework does this.**

---

## Performance Numbers

### Local Message Passing

- Tokio mpsc: **200ns**
- Actix: **250ns**
- Kameo/Ractor: **300ns**
- Coerce: **350ns**

**Target for HFT**: < 500ns ✅ All frameworks meet this

**BUT**: Serialization adds 200-300ns overhead
- With Arc<T>: 200ns
- With serialization: 500ns
- **Custom saves 300ns per message**

### Zero-Copy Serialization (rkyv)

- rkyv deserialize: **2-4x faster** than bincode
- Read throughput: **4.0 GB/s** vs 2.1 GB/s (bincode)
- Critical for remote messaging

---

## Implementation Estimate

### Code Size
```
mycelium-core:       ~1,500 lines  (Actor, supervision)
mycelium-transport:  ~2,000 lines  (Arc/Unix/TCP)
mycelium-protocol:   ~1,000 lines  (rkyv, TLV)
────────────────────────────────────────────
Total:               ~4,500 lines
```

### Timeline
```
Week 1-2:   MVP (local actors + supervision)        ← Start here
Week 3-4:   Local ActorRef (Arc<T>)                 ← Production-ready local
Week 5-6:   rkyv serialization
Week 7-8:   Unix socket transport                   ← Multi-process ready
Week 9-12:  TCP transport                           ← Distributed ready
```

**MVP in 4 weeks, full system in 12 weeks**

---

## Core Pattern (Tokio Native)

```rust
// Actor = Task + Handle
struct MyActor { 
    rx: mpsc::Receiver<Msg>,
    state: State,
}

#[derive(Clone)]
struct MyActorHandle { 
    tx: mpsc::Sender<Msg> 
}

// Spawn
fn spawn() -> MyActorHandle {
    let (tx, rx) = mpsc::channel(32);  // Bounded!
    let actor = MyActor { rx, state: State::default() };
    tokio::spawn(async move { actor.run().await });
    MyActorHandle { tx }
}

// Use
handle.send(msg).await?;
```

**200 lines for basic actor. Simple.**

---

## Key Decisions

### 1. Adaptive Transport (Custom Only)

```rust
pub enum ActorRef<T> {
    Local(Arc<LocalRef>),      // Zero serialization
    UnixSocket(UnixRef),       // Serialize once
    Tcp(TcpRef),               // Full serialization
}
```

### 2. Zero-Copy Messages

```rust
// Local: Arc<T>
local_ref.send(Arc::new(msg)).await?;

// Remote: rkyv (zero-copy deserialize)
let bytes = rkyv::to_bytes(&msg)?;
remote_ref.send(bytes).await?;
let archived = rkyv::check_archived_root(&bytes)?;  // No allocation!
```

### 3. Supervision Trees (Ractor-Inspired)

```rust
let supervisor = Supervisor::new(OneForOne);
supervisor.add_child(actor_id, spawn_fn, Permanent);
// Auto-restarts on panic
```

### 4. Bounded Mailboxes

```rust
let (tx, rx) = mpsc::channel(32);  // Backpressure at 32 messages
```

---

## Critical Dos and Don'ts

### Do ✅

- Use **bounded channels** (backpressure)
- Use **Arc<T>** for local messaging (zero-copy)
- Use **rkyv** for remote (zero-copy deserialize)
- Use **try_send** for critical actors (fail fast)
- **Offload CPU work** to spawn_blocking

### Don't ❌

- **Unbounded mailboxes** (memory leaks)
- **Cycles with bounded channels** (deadlock)
- **Block Tokio event loop** (kills performance)
- **Serialize local messages** (unnecessary overhead)

---

## Risk Assessment

### Implementation Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Takes too long | Medium | Medium | MVP in 4 weeks, phased delivery |
| Maintenance burden | Low | Low | Simple codebase, good tests |
| Missing features | Low | Low | Build what's needed only |
| Critical bugs | Medium | High | Proven patterns, extensive testing |

**Overall**: Medium-Low risk, acceptable for HFT requirements

---

## Validation Checklist

Before starting, confirm:

- [ ] **Adaptive transport needed?** (If yes → custom required)
- [ ] **Latency targets?** (Sub-microsecond → custom required)
- [ ] **Team capacity?** (12 weeks or phased?)
- [ ] **Start with MVP?** (Local only first, add transport later)

**All checked?** → Proceed with custom implementation

---

## Production Examples

### Meta (Ractor)
- Uses Ractor for **control plane** (overload protection)
- NOT for **data plane** (message hot path)
- Lesson: Framework for coordination, custom for latency

### Goldman Sachs (Erlang)
- Erlang for **coordination**
- C++ for **hot path**
- Lesson: Same pattern

### HFT Industry
- Custom implementations standard
- Framework overhead unacceptable
- Sub-microsecond requires full control

**Mycelium follows industry pattern.**

---

## Next Steps

### 1. Review Research (30 min)
- [Decision Summary](./ACTOR_FRAMEWORK_DECISION_SUMMARY.md) - Executive brief
- [Full Comparison](./ACTOR_FRAMEWORK_COMPARISON.md) - Deep dive (optional)

### 2. Validate Requirements (1 hour)
- Confirm adaptive transport needed
- Clarify latency targets
- Define deployment model

### 3. Prototype (1 week, optional)
- Basic Tokio actor
- Benchmark vs Actix
- Validate assumptions

### 4. Start MVP (Week 1-4)
- Follow [Implementation Guide](./CUSTOM_ACTOR_IMPLEMENTATION_GUIDE.md)
- Basic actors + supervision
- Local messaging with Arc<T>

---

## Key Resources

### Documentation
- **Alice Ryhl's Guide**: https://ryhl.io/blog/actors-with-tokio/
- **Tokio Tutorial**: https://tokio.rs/tokio/tutorial/channels
- **Ractor Docs**: https://slawlor.github.io/ractor/ (for supervision patterns)

### Performance
- **rkyv**: https://rkyv.org/
- **Rust Serialization Benchmarks**: https://github.com/djkoloski/rust_serialization_benchmark

### Comparison
- **Framework Comparison**: https://tqwewe.com/blog/comparing-rust-actor-libraries/

---

## One-Sentence Summary

**Build custom Tokio-based actors because no framework supports adaptive transport (Arc<T> for local, sockets for remote), which is critical for HFT performance.**

---

## Questions?

1. **Why not use Ractor?** - Excellent supervision, but no Arc<T> optimization and cluster not production-ready
2. **Why not use Actix?** - Fast locally, but no distributed support
3. **Why not use Kameo?** - libp2p overhead too heavy for HFT
4. **Is custom too risky?** - No, it's HFT industry standard and only ~4,500 lines
5. **Can we change later?** - Yes, start with MVP, validate assumptions

**Still unsure?** → Read [Decision Summary](./ACTOR_FRAMEWORK_DECISION_SUMMARY.md)

---

**Bottom line: Custom implementation is the right choice for Mycelium's HFT requirements.**
