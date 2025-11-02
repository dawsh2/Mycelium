# Mycelium Actor Framework Research

**Research Date**: November 2, 2025  
**Status**: Complete  
**Recommendation**: Build custom Tokio-based actor system

---

## Executive Summary

After comprehensive research of the Rust actor ecosystem (Actix, Kameo, Ractor, Coerce, Xtra, Bastion, and others), **no existing framework meets Mycelium's requirements** for high-frequency trading.

**Core Gap**: No framework supports **adaptive transport** (Arc<T> for local, Unix sockets for multi-process, TCP for distributed with same API).

**Recommendation**: Build a thin custom layer (~4,500 lines) on top of Tokio, following proven patterns and borrowing best practices from existing frameworks.

---

## Documents in This Research

### 1. [ACTOR_FRAMEWORK_COMPARISON.md](./ACTOR_FRAMEWORK_COMPARISON.md)
**Comprehensive 15,000+ word analysis** of all major Rust actor frameworks.

**Covers:**
- Detailed framework-by-framework comparison
- Performance benchmarks (latency, spawn time)
- Feature comparison tables
- Real-world production usage
- HFT-specific requirements analysis
- Zero-copy serialization (rkyv vs bincode)
- Erlang actor model in finance

**Read this for**: Deep understanding of each framework's strengths/weaknesses.

### 2. [CUSTOM_ACTOR_IMPLEMENTATION_GUIDE.md](./CUSTOM_ACTOR_IMPLEMENTATION_GUIDE.md)
**Practical implementation guide** with code examples.

**Covers:**
- Core Tokio actor pattern (task + handle)
- Adaptive transport implementation
- Supervision trees (Erlang-style)
- Zero-copy messaging (Arc<T> + rkyv)
- Performance optimizations
- Common patterns and pitfalls
- Testing strategies
- Benchmarking approaches

**Read this for**: How to actually build the system.

### 3. [ACTOR_FRAMEWORK_DECISION_SUMMARY.md](./ACTOR_FRAMEWORK_DECISION_SUMMARY.md)
**Executive decision brief** with clear recommendations.

**Covers:**
- Quick comparison table
- Why no framework works
- Risk analysis with mitigation
- Implementation timeline
- Success criteria
- Decision approval checklist

**Read this for**: Quick understanding and decision-making.

---

## Key Findings

### 1. Framework Comparison

| Framework | Status | Local Latency | Adaptive Transport | HFT Ready |
|-----------|--------|---------------|-------------------|-----------|
| Actix | Active | ~200ns | ❌ | Partial |
| Kameo | Very Active | ~300ns | ⚠️ libp2p | Moderate |
| Ractor | Active (Meta) | ~300ns | ⚠️ Beta cluster | Moderate |
| Coerce | Active | ~350ns | ⚠️ Always serializes | Poor |
| Xtra | Active | ~300ns | ❌ | Limited |
| Bastion | Abandoned | Unknown | Unknown | No |
| Tokio Native | Official | ~200ns | ✅ DIY | ✅ Yes |

### 2. Critical Gap: Adaptive Transport

**What Mycelium needs:**
```rust
// Same API, different transports based on deployment
let actor: ActorRef<Msg> = ...;  // Could be Local, Unix, or TCP
actor.send(msg).await?;           // Zero-copy if local, serialized if remote
```

**What frameworks provide:**
- Actix: Local only (no remote)
- Kameo/Ractor/Coerce: Always serialize (even local)
- None: Arc<T> optimization for local actors

**Impact on HFT:**
- Local with Arc<T>: ~200ns
- Local with serialization: ~500ns
- Difference: 300ns per message
- At 1M msg/sec: 30% overhead

**Conclusion**: Must build custom to get adaptive transport.

### 3. Zero-Copy Serialization: rkyv

**Performance comparison:**

| Operation | rkyv | bincode | Improvement |
|-----------|------|---------|-------------|
| Serialize | Fast | Fast | Similar |
| Deserialize | Zero-copy | Must reconstruct | 2-4x faster |
| Read throughput | 4.0 GB/s | 2.1 GB/s | 1.9x |
| Access time | Direct memory | Object creation | Significantly faster |

**For HFT**: rkyv's zero-copy deserialization is critical for remote messaging.

### 4. Production HFT Patterns

**Research shows:**
- ✅ Custom actor implementations standard in HFT
- ✅ Frameworks used for coordination, not hot path
- ✅ Meta uses Ractor for control plane (not data plane)
- ✅ Goldman Sachs: Erlang coordination + C++ hot path
- ✅ Sub-microsecond targets require custom solutions

**Pattern**:
```
Control Plane (Actor Framework)
  ├── Strategy coordination
  ├── Risk management
  └── Monitoring

Data Plane (Custom, Zero-Copy)
  ├── Market data ingestion
  ├── Order execution
  └── Arbitrage detection
```

### 5. Best Practices (From Research)

**Do:**
- ✅ Use bounded channels (backpressure)
- ✅ Arc<T> for local messaging (zero-copy)
- ✅ rkyv for remote messaging (zero-copy deserialize)
- ✅ Supervision trees (fault tolerance)
- ✅ try_send for critical actors (fail fast)

**Don't:**
- ❌ Unbounded mailboxes (memory leaks)
- ❌ Cycles with bounded channels (deadlock)
- ❌ Block Tokio event loop (use spawn_blocking)
- ❌ Serialize for local messaging (unnecessary overhead)

---

## Recommendation Details

### What to Build

**Custom Tokio-based actor system with:**
1. `ActorRef<T>` enum (Local/Unix/TCP)
2. Supervision trees (Ractor-inspired)
3. Actor registry with discovery
4. Message trait (Arc<T> + rkyv)
5. NUMA-aware placement

**Code estimate**: ~4,500 lines
**Timeline**: 12 weeks (4 weeks for MVP)

### Why Not Use a Framework?

**Dealbreakers:**
1. ❌ No adaptive transport support
2. ❌ Always serialize (even local actors)
3. ❌ Framework overhead for hot path
4. ❌ Can't optimize Arc<T> fast path

**Benefits of custom:**
1. ✅ Adaptive transport (core requirement)
2. ✅ Zero overhead for local actors
3. ✅ Full control over serialization
4. ✅ HFT-optimized by design
5. ✅ No framework limitations

### Implementation Strategy

**Phase 1: MVP (Week 1-4)**
- Basic Tokio actor pattern
- Supervision trees
- Local messaging with Arc<T>
- Actor registry

**Phase 2: Serialization (Week 5-6)**
- rkyv integration
- Message trait
- Zero-copy testing

**Phase 3: Multi-Process (Week 7-8)**
- Unix socket transport
- Same-machine coordination

**Phase 4: Distributed (Week 9-12)**
- TCP transport
- Discovery protocol
- Distributed supervision

---

## Validation Metrics

### Success Criteria

**Performance:**
- [ ] Local messaging: < 250ns per message
- [ ] Unix socket: < 2μs per message
- [ ] TCP: < 10μs per message (LAN)
- [ ] Zero-copy: rkyv deserialization < 100ns

**Functionality:**
- [ ] Supervision: actors restart on panic
- [ ] Backpressure: bounded mailboxes work
- [ ] Discovery: actors found by name
- [ ] Transport: same API for all transports

**Production-Ready:**
- [ ] Comprehensive tests (unit + integration)
- [ ] Chaos testing (random failures)
- [ ] Monitoring hooks (metrics, tracing)
- [ ] Documentation (API + patterns)

### Benchmarks to Run

1. **vs Actix** (local messaging)
   - Target: Within 20% of Actix
   - Validates Tokio baseline performance

2. **vs Ractor** (spawn time)
   - Target: Comparable spawn times
   - Validates actor creation overhead

3. **rkyv vs bincode** (serialization)
   - Target: 2x faster deserialization
   - Validates zero-copy benefits

4. **Arc<T> vs serialization** (local)
   - Target: 2-3x faster without serialization
   - Validates adaptive transport value

---

## Risk Assessment

### Technical Risks

**Risk**: Implementation takes too long  
**Mitigation**: MVP in 4 weeks, incremental delivery  
**Impact**: Medium

**Risk**: Maintenance burden  
**Mitigation**: Simple focused codebase, comprehensive tests  
**Impact**: Low

**Risk**: Missing features  
**Mitigation**: Build only what's needed, add incrementally  
**Impact**: Low

**Risk**: Bugs in critical system  
**Mitigation**: Based on proven patterns, extensive testing  
**Impact**: Medium

### Decision Risks

**Risk**: Framework actually would work  
**Mitigation**: 1-week prototype to validate assumptions  
**Impact**: High (wasted time)

**Risk**: Requirements change  
**Mitigation**: Start with MVP, adapt based on learning  
**Impact**: Medium

### Risk Mitigation Summary

**Overall risk**: Medium-Low

**Why acceptable:**
- Based on proven Tokio patterns
- Borrow from mature frameworks (Ractor, Erlang)
- Incremental delivery reduces risk
- Team owns the critical path
- No framework lock-in

---

## Questions Before Starting

### Critical Questions

1. **Is adaptive transport actually needed?**
   - If always distributed: Frameworks might work
   - If local optimization critical: Custom required

2. **What are real latency targets?**
   - Sub-millisecond: Any framework works
   - Sub-100μs: Careful design needed
   - Sub-1μs: Custom implementation required

3. **What's deployment model?**
   - Always distributed: Consider frameworks
   - Start local, scale distributed: Custom required
   - NUMA-aware single machine: Custom required

4. **Team capacity?**
   - 1 engineer for 12 weeks?
   - 2 engineers for 6 weeks?
   - Part-time alongside other work?

### Validation Questions

1. **Can we prototype in 1 week?**
   - Basic Tokio actor + benchmark
   - Validates performance assumptions
   - Low-risk investment

2. **Do we need all transports now?**
   - Start with local only (4 weeks)
   - Add Unix/TCP when needed
   - Reduces initial scope

3. **What's MVP really need?**
   - Just local actors + supervision?
   - Or full distributed from day 1?
   - Affects timeline

---

## Next Steps

### Immediate Actions

1. **Review research docs** (this folder)
   - Read decision summary first
   - Deep dive comparison if needed
   - Study implementation guide

2. **Validate requirements**
   - Confirm adaptive transport is needed
   - Clarify latency targets
   - Define deployment model

3. **Prototype (optional but recommended)**
   - 1 week basic Tokio actor
   - Benchmark vs Actix
   - Validate assumptions

4. **Get approval**
   - Review with stakeholders
   - Confirm timeline/resources
   - Align on MVP scope

5. **Start implementation**
   - Follow implementation guide
   - Begin with Phase 1 (MVP)
   - Iterate based on feedback

### Timeline

```
Week 0:     Review research + prototype           [Current]
Week 1-2:   Basic Tokio actors + supervision      [MVP starts]
Week 3-4:   Local ActorRef with Arc<T>            [MVP complete]
Week 5-6:   rkyv serialization
Week 7-8:   Unix socket transport
Week 9-12:  TCP transport + discovery             [Full system]
Week 13-16: Production hardening                  [Optional]
```

---

## Research Methodology

### Frameworks Evaluated

1. **Actix** - Most popular Rust actor framework
2. **Kameo** - Modern distributed actor framework
3. **Ractor** - Erlang-inspired, Meta production
4. **Coerce** - Location-transparent distributed
5. **Xtra** - Lightweight, multi-runtime
6. **Bastion** - Fault-tolerant runtime (abandoned)
7. **Riker** - Actor framework (dormant)
8. **Axiom** - Cluster framework (unclear status)
9. **Tokio Native** - DIY actor patterns

### Research Sources

- **Framework Documentation** (official docs)
- **Benchmark Comparison** (tqwewe.com/blog)
- **Performance Data** (GitHub benchmarks)
- **Production Usage** (Meta RustConf 2024, HN discussions)
- **Best Practices** (Alice Ryhl's blog, Tokio tutorials)
- **HFT Context** (Rust HFT articles, Erlang finance usage)
- **Serialization** (rkyv benchmarks, rust_serialization_benchmark)

### Validation Methods

- ✅ Cross-referenced multiple sources
- ✅ Checked GitHub activity/maintenance
- ✅ Reviewed production usage examples
- ✅ Analyzed benchmark methodologies
- ✅ Studied HFT industry patterns
- ✅ Consulted Tokio official patterns

---

## Appendix: Framework Deep Dives

### Actix: Fast but Local-Only

**Best for**: Single-process high-concurrency
**Skip if**: Need distributed actors
**Key insight**: Being de-emphasized as async/await matured

### Kameo: Modern but Network-Heavy

**Best for**: Distributed systems with libp2p
**Skip if**: HFT latency targets, local optimization
**Key insight**: libp2p overhead too heavy for HFT

### Ractor: Excellent Supervision

**Best for**: Erlang-style fault tolerance
**Skip if**: Need production-ready clustering
**Key insight**: Meta uses for control plane, not data plane

### Coerce: Location-Transparent

**Best for**: Mixed local/remote without caring
**Skip if**: Need local optimization
**Key insight**: Always serializes, kills local performance

### Xtra: Simple but Limited

**Best for**: Basic local concurrency
**Skip if**: Need supervision or distribution
**Key insight**: Too minimal for production HFT

### Tokio Native: Full Control

**Best for**: Custom requirements, performance-critical
**Skip if**: Want batteries-included framework
**Key insight**: HFT industry standard approach

---

## Conclusion

**The research is clear: build custom on Tokio.**

No existing framework meets Mycelium's unique requirements for adaptive transport and HFT-level performance. The custom implementation is manageable (4,500 lines, 12 weeks) and follows proven patterns.

**This is the right decision for Mycelium.**

---

## Contact

For questions about this research:
- Review full analysis in `ACTOR_FRAMEWORK_COMPARISON.md`
- Check implementation guide in `CUSTOM_ACTOR_IMPLEMENTATION_GUIDE.md`
- Read decision brief in `ACTOR_FRAMEWORK_DECISION_SUMMARY.md`

**All documents in**: `/Users/daws/repos/mycelium/docs/research/`
