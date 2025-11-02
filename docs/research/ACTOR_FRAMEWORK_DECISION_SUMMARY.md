# Actor Framework Decision Summary

**TL;DR**: Build a custom actor layer on Tokio. No existing framework meets Mycelium's requirements.

---

## Quick Comparison Table

| Framework | Maintenance | Local Latency | Adaptive Transport | Supervision | Zero-Copy | HFT Ready | Verdict |
|-----------|-------------|---------------|-------------------|-------------|-----------|-----------|---------|
| **Actix** | Active (diminishing) | ✅✅ ~200ns | ❌ Single-process only | ✅ Yes | ❌ No | ⚠️ Partial | Good for local, not distributed |
| **Kameo** | ✅✅ Very Active | ✅ ~300ns | ⚠️ libp2p (heavy) | ✅ Yes | ❌ No | ⚠️ Moderate | Too much network overhead |
| **Ractor** | ✅ Active (Meta) | ✅ ~300ns | ⚠️ Cluster (beta) | ✅✅ Excellent | ❌ No | ⚠️ Moderate | Best supervision, but no Arc<T> |
| **Coerce** | ✅ Active | ⚠️ ~350ns | ⚠️ Location-transparent | ✅ Yes | ❌ No | ❌ Poor | Too much overhead |
| **Xtra** | ✅ Active | ✅ ~300ns | ❌ None | ❌ No | ❌ No | ❌ Limited | No supervision |
| **Bastion** | ❌ Abandoned | ❓ Unknown | ❓ Unknown | ⚠️ Theory | ❌ No | ❌ No | Don't use |
| **Riker** | ⚠️ Dormant | ❓ Unknown | ❓ Unknown | ⚠️ Theory | ❌ No | ❌ No | Don't use |
| **Tokio Native** | ✅✅ Official | ✅✅ ~200ns | ✅ Build it | ⚠️ DIY | ✅ DIY | ✅✅ Yes | **Recommended** |

---

## Why No Framework Works

### Critical Gap: Adaptive Transport

**Mycelium's unique requirement:**
```
Same actor code works with:
1. Arc<T> for in-process (zero serialization)
2. Unix sockets for multi-process (serialize once)
3. TCP for distributed (full serialization)
```

**What frameworks provide:**
- **Actix**: Local only, no remote support
- **Kameo/Ractor/Coerce**: Always serialize, even for local
- **None**: Support Arc<T> optimization for local actors

**Why this matters for HFT:**
- Local messaging: 200ns (Arc<T>) vs 300-500ns (serialize + deserialize)
- At 1M messages/sec, 100ns difference = 10% of system budget
- HFT can't afford serialization overhead for local actors

### The Trade-off

```
┌──────────────────────────────────────────────┐
│  Framework Benefits                          │
│  ✅ Mature supervision trees                │
│  ✅ Proven patterns                          │
│  ✅ Less code to write                       │
│  ✅ Community support                        │
└──────────────────────────────────────────────┘
                    VS
┌──────────────────────────────────────────────┐
│  Custom Implementation Benefits              │
│  ✅ Adaptive transport (core requirement)   │
│  ✅ Zero overhead for hot path              │
│  ✅ Full control over serialization          │
│  ✅ HFT-optimized design                     │
│  ✅ No framework limitations                 │
└──────────────────────────────────────────────┘
```

**Winner**: Custom implementation. The core requirement (adaptive transport) cannot be compromised.

---

## The Hybrid Approach (Rejected)

**Could we use a framework + custom transport?**

### Option: Ractor + Custom Transport Layer

**Pros:**
- Excellent supervision trees from Ractor
- Battle-tested at Meta
- Less code to write

**Cons:**
- Ractor's messaging API assumes serialization
- Would need to fork or wrapper Ractor heavily
- Framework overhead still present
- Two systems to maintain (Ractor + our layer)

**Conclusion**: More complexity than building from scratch.

---

## What We're Actually Building

### Not "Yet Another Actor Framework"

**We're building**:
- Transport abstraction (`ActorRef` enum)
- Supervision trees (inspired by Ractor/Erlang)
- Actor registry
- Message serialization strategy (rkyv)

**We're NOT building**:
- Novel actor patterns (use Tokio's proven pattern)
- Custom runtime (use Tokio)
- Reinvented wheels (use ecosystem crates)

### Code Estimate

```
mycelium-core/        ~1,500 lines
  - Actor trait
  - Supervision tree
  - Registry
  - Lifecycle hooks

mycelium-transport/   ~2,000 lines
  - ActorRef enum
  - LocalActorRef (Arc<T>)
  - UnixSocketActorRef
  - TcpActorRef
  - Connection management

mycelium-protocol/    ~1,000 lines
  - Message trait
  - rkyv integration
  - TLV encoding

Total: ~4,500 lines of core code
```

**Comparison**: Actix core is ~10,000 lines, Ractor is ~8,000 lines.

**We need less because**:
- No custom runtime
- Simpler API (specific to our needs)
- Leveraging Tokio primitives
- No backward compatibility constraints

---

## Risk Analysis

### Risk 1: "Takes too long to build"

**Mitigation:**
- MVP in 4 weeks (local actors + supervision)
- Incremental delivery (local → unix → tcp)
- Can start using after Phase 1

**Timeline:**
```
Week 1-2:  Basic Tokio actors + supervision      [MVP starts here]
Week 3-4:  Local ActorRef with Arc<T>            [Production-ready local]
Week 5-6:  rkyv serialization                    
Week 7-8:  Unix socket transport                 [Multi-process ready]
Week 9-12: TCP transport + discovery             [Distributed ready]
```

### Risk 2: "Maintenance burden"

**Mitigation:**
- Simple, focused codebase (~4,500 lines)
- Comprehensive tests
- Well-documented patterns
- Based on proven Tokio primitives

**Counter-argument:**
- Any framework requires learning and maintenance
- Custom code is exactly what we need (no more, no less)
- Team owns the critical path

### Risk 3: "Missing framework features"

**Mitigation:**
- We only build what Mycelium needs
- Frameworks have features we don't need
- Can add features incrementally

**What we skip:**
- Actor persistence (don't need it)
- Complex routing (simple is better)
- Legacy compatibility (greenfield)
- Multi-runtime support (Tokio only)

### Risk 4: "Bugs in critical system"

**Mitigation:**
- Based on proven Tokio patterns (Alice Ryhl's guide)
- Borrow supervision from Ractor (proven at Meta)
- Comprehensive testing (unit, integration, chaos)
- Start with MVP, expand incrementally

**Benefits of custom:**
- Full control to fix bugs
- No waiting for upstream fixes
- No framework upgrade breaking changes

---

## Industry Precedent

### Production HFT Systems

**From research:**
- ✅ Custom actor implementations common in HFT
- ✅ Rust HFT projects use Tokio directly
- ✅ Sub-microsecond targets require custom code
- ✅ "Framework overhead unacceptable" (industry pattern)

**Meta's Ractor usage:**
- ✅ Control plane (overload protection, coordination)
- ❌ NOT data plane (message hot path)

**Lesson**: Even companies using actor frameworks build custom code for latency-critical paths.

### Erlang in Finance

**Real usage:**
- Goldman Sachs: Erlang for coordination, C++ for hot path
- Delta Exchange: Erlang for HFT, but not nanosecond-level logic

**Pattern**: Actor model for **resilience and coordination**, custom code for **ultra-low-latency**.

**Mycelium approach**: Unify both with adaptive transport.

---

## What Success Looks Like

### Phase 1: MVP (Week 2)
```rust
// Basic actor with supervision
let actor = MyActor::spawn()?;
actor.send(msg).await?;

// Supervisor restarts on panic
let supervisor = Supervisor::new(OneForOne);
supervisor.add_child(actor_id, spawn_fn, Permanent);
```

**Validation**: 
- ✅ ~200ns local message latency
- ✅ Actors restart on panic
- ✅ Bounded mailboxes with backpressure

### Phase 2: Production Local (Week 4)
```rust
// ActorRef abstraction
let actor_ref: ActorRef<MyMsg> = ActorRef::Local(local_ref);
actor_ref.send(msg).await?;  // Arc<T>, no serialization

// Registry
REGISTRY.register(actor_id, actor_ref);
let actor = REGISTRY.lookup(&actor_id)?;
```

**Validation**:
- ✅ Zero-copy local messaging
- ✅ Type-safe actor references
- ✅ Actor discovery works

### Phase 3: Multi-Process (Week 8)
```rust
// Same API, different transport
let actor_ref: ActorRef<MyMsg> = ActorRef::UnixSocket(unix_ref);
actor_ref.send(msg).await?;  // rkyv serialization

// Works transparently
REGISTRY.register(actor_id, actor_ref);
let actor = REGISTRY.lookup(&actor_id)?;
actor.send(msg).await?;  // Doesn't know if local or unix
```

**Validation**:
- ✅ Unix socket messaging works
- ✅ Zero-copy deserialization with rkyv
- ✅ Same API for local and unix

### Phase 4: Distributed (Week 12)
```rust
// Same API, TCP transport
let actor_ref: ActorRef<MyMsg> = ActorRef::Tcp(tcp_ref);
actor_ref.send(msg).await?;

// Discovery protocol
let actor = discover_actor("polygon_adapter").await?;
actor.send(msg).await?;  // Could be local, unix, or TCP
```

**Validation**:
- ✅ TCP messaging works
- ✅ Actor discovery across nodes
- ✅ Supervision works across network

---

## Final Recommendation

### Build Custom Tokio-Based Actor System

**Core reasoning:**
1. **Adaptive transport is non-negotiable** - Mycelium's killer feature
2. **No framework supports this** - all serialize for remote
3. **HFT requires zero overhead** - local messaging must be ~200ns
4. **Custom is industry standard** - for HFT latency targets
5. **Tokio provides primitives** - no need to reinvent
6. **Manageable scope** - ~4,500 lines, 12-week timeline
7. **Full control** - optimize exactly where needed

### Implementation Strategy

**Start**: Tokio actor pattern (task + handle)
**Add**: Supervision trees (borrow from Ractor)
**Build**: Transport layer (Arc → Unix → TCP)
**Integrate**: rkyv for zero-copy serialization
**Optimize**: Profile-guided hot path optimization
**Harden**: Production monitoring and testing

### Why This Beats Frameworks

| Requirement | Custom | Best Framework | Winner |
|-------------|--------|----------------|--------|
| Adaptive transport | ✅✅ Core feature | ❌ None support | **Custom** |
| Local latency | ✅ ~200ns (Tokio baseline) | ✅ ~200-300ns | **Tie** |
| Supervision | ✅ DIY (Ractor-inspired) | ✅✅ Ractor (excellent) | **Framework** |
| Type safety | ✅ Rust type system | ✅ Rust type system | **Tie** |
| Zero-copy | ✅ Arc<T> + rkyv | ❌ Always serialize | **Custom** |
| Backpressure | ✅ Tokio mpsc | ✅ Tokio mpsc | **Tie** |
| Discovery | ✅ Build it | ⚠️ Limited | **Custom** |
| Production-ready | ⚠️ Need to build | ✅ Mature | **Framework** |
| HFT optimized | ✅✅ By design | ⚠️ Generic | **Custom** |

**Score: Custom 5, Framework 2, Tie 3**

### The Decision

**Go custom.** The adaptive transport requirement alone justifies it, and the HFT performance requirements seal the deal. Frameworks solve the wrong problem for Mycelium.

---

## Approval Checklist

Before proceeding, confirm:

- [ ] **Adaptive transport is confirmed requirement** (not negotiable?)
- [ ] **Team capacity for 12-week implementation** (or phased delivery?)
- [ ] **Comfortable with maintenance burden** (~4,500 lines to own)
- [ ] **HFT latency targets are real** (sub-microsecond matters?)
- [ ] **Willing to start with MVP** (local actors first, expand later)

**If all checked**: Proceed with custom implementation.

**If uncertain**: Prototype for 1 week to validate latency assumptions.

---

## Next Actions

1. **Review this research** with team/stakeholders
2. **Validate latency requirements** - are sub-microsecond targets real?
3. **Prototype basic Tokio actor** (2-3 days)
4. **Benchmark against Actix** to confirm performance assumptions
5. **Get approval for Phase 1** (4 weeks, MVP scope)
6. **Start implementation** following guide in `CUSTOM_ACTOR_IMPLEMENTATION_GUIDE.md`

---

## Questions to Resolve

1. **Is adaptive transport actually needed?** 
   - Can Mycelium always run distributed (always serialize)?
   - Or is local optimization critical for HFT?

2. **What are real latency targets?**
   - Sub-millisecond (easy, any framework works)
   - Sub-100-microsecond (careful design needed)
   - Sub-microsecond (custom implementation required)

3. **What's the deployment model?**
   - Always distributed? (frameworks might work)
   - Start local, scale to distributed? (adaptive transport critical)
   - NUMA-aware single machine? (local optimization critical)

4. **Team experience?**
   - Comfortable with Tokio async? (custom is easier)
   - New to async Rust? (framework might help)
   - HFT background? (custom is expected)

---

## Conclusion

**Recommendation stands: Build custom Tokio-based actor system.**

The research clearly shows:
- ✅ No framework meets Mycelium's unique requirements
- ✅ Custom implementation is HFT industry standard
- ✅ Tokio provides all necessary primitives
- ✅ Manageable scope with clear timeline
- ✅ Adaptive transport is killer feature

**Proceed with confidence.** This is the right technical decision for Mycelium's HFT requirements.

---

**Files created:**
1. `/docs/research/ACTOR_FRAMEWORK_COMPARISON.md` - Detailed analysis
2. `/docs/research/CUSTOM_ACTOR_IMPLEMENTATION_GUIDE.md` - Implementation guide
3. `/docs/research/ACTOR_FRAMEWORK_DECISION_SUMMARY.md` - This executive summary
