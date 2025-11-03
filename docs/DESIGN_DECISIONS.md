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

**4. Zerocopy Trait Generation**

Build script generates manual unsafe impls for zerocopy:

```rust
// Generated for U256 fields (not auto-derivable)
unsafe impl zerocopy::AsBytes for PoolStateUpdate {
    fn only_derive_is_allowed_to_implement_this_trait() {}
}
```

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

## Summary

**Core philosophy:** Optimize for correctness, maintainability, and flexibility first. Micro-optimize only when profiling shows bottlenecks.

**Key decisions:**
1. **Envelope abstraction** → Worth 1-5ns for metadata, uniformity, and smaller binaries
2. **Runtime transport selection** → Worth 1-2 cycles for single-binary deployment
3. **Schema-driven codegen** → Worth build complexity for type safety and consistency

**Performance optimization order:**
1. Channel tuning
2. Remove unused features (sequence tracking)
3. Object pooling
4. Lock-free channels
5. Batch operations

**Only then** consider architectural changes like full monomorphization.

---

**Last Updated:** 2025-01-03
**Reviewers:** @daws
