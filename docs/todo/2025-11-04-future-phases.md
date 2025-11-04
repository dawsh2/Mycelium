# Future Phases - Deferred

**Date**: 2025-11-04  
**Status**: Deferred until needed  
**Reason**: Phases 1-2 are complete and sufficient to start building real applications

## Phase 1.5: Backpressure Monitoring (~3 days)

**What it would add:**
- `ctx.queue_depth()` - Returns current queue size
- `ctx.backpressure_threshold_exceeded()` - Returns true when >80% capacity
- Example showing service handling backpressure by sampling/dropping messages

**Why it's deferred:**
- Don't know if backpressure will be an issue yet
- Trivial to add when actually needed (15-30 min implementation)
- Real use cases will reveal if this is necessary
- Premature optimization

**When to implement:**
- When services actually get overwhelmed with messages
- When queue depths grow unbounded
- When profiling shows message buildup

**Implementation notes:**
- Add queue depth tracking to ChannelManager
- Expose via ServiceContext
- Add threshold config to ServiceRuntime

---

## Phase 3: Performance Optimization (~1 week)

**What it would add:**

### Benchmarks
- Comprehensive benchmarks for Unix transport
- Comprehensive benchmarks for TCP transport
- End-to-end latency measurements
- Throughput measurements under load

### Optimizations
- Publisher caching in ServiceContext (saves 40ns)
- Feature flags for optional metrics (saves 40ns)
- Both together could get to 47ns (same as raw publish)

### Documentation
- Performance guide
- Tuning recommendations
- API reference documentation

**Why it's deferred:**
- Current performance is already excellent (120ns, 8M/sec)
- 40% faster than design target already
- Optimizations add complexity for marginal gains
- Don't optimize before knowing real bottlenecks

**When to implement:**
- When profiling shows `ctx.emit()` is a bottleneck (unlikely)
- When building extremely high-frequency services (>1M events/sec per core)
- When 120ns is measured to be too slow in production
- For comprehensive documentation before open-sourcing

**Implementation notes:**
- Publisher caching: Use DashMap<TypeId, Arc<Publisher<T>>>
- Feature flags: Add `#[cfg(feature = "metrics")]` around timing code
- Benchmarks: Use criterion for statistical rigor

---

## Phase 4: Multi-Service Example (~3 days)

**What it would add:**
- End-to-end pipeline example (3+ services)
- Demonstrates cross-service communication
- Shows Unix socket transport in action
- Load testing results
- Trace propagation demonstration (if Phase 2.5 done)

**Why it's deferred:**
- Real applications (Bandit) will be better examples
- Transport layer already works (72/72 tests passing)
- Better to learn from real use cases than contrived examples
- Can validate cross-service patterns while building Bandit

**When to implement:**
- After building first real multi-service system (Bandit)
- When creating tutorial/documentation for others
- When open-sourcing and need example code

**Possible examples:**
- Trading pipeline: MarketData → Strategy → Execution
- Game server: WorldState → Physics → Networking
- Analytics: Ingest → Transform → Aggregate → Dashboard

---

## Phase 2.5: Wire Format Trace Propagation (~2 days) - OPTIONAL

**What it would add:**
- Extend TLV header with trace_id (16 bytes) + span_id (8 bytes)
- Automatic trace context serialization in Unix/TCP transports
- Auto-create child spans in receiving services
- Full distributed tracing across service boundaries

**Why it's deferred:**
- Current in-memory (Arc) transport already propagates Envelope with trace_id
- Wire format changes require coordination with transport layer
- Nice-to-have for distributed tracing, not critical
- Logs already include trace_id for correlation

**When to implement:**
- When building distributed multi-service systems
- When integrating with Jaeger/Zipkin for distributed tracing
- When cross-service trace correlation becomes important
- After Phase 4 examples show the need

**Implementation notes:**
- Update TLV codec to include trace context in header
- Modify Unix/TCP transports to serialize trace_id + span_id
- Update ServiceContext to extract and propagate trace context
- Ensure backward compatibility with old wire format

---

## Recommended Next Steps (Instead of These Phases)

**Start building Bandit services using Mycelium:**

1. **PolygonAdapter** - Ingest blockchain events
   - Connect to Polygon WebSocket
   - Parse swap events
   - Emit structured messages via `ctx.emit()`
   - Discover what works and what doesn't

2. **MetadataService** - Enrich with token metadata
   - Subscribe to swap events
   - Look up token metadata (Redis/Postgres)
   - Emit enriched events

3. **ArbitrageDetector** - Find opportunities
   - Subscribe to enriched swaps
   - Calculate arbitrage opportunities
   - Emit signals

**Why this is better:**
- Reveals real requirements vs. hypothetical ones
- Discovers actual pain points
- Validates the API design with real use
- Shows if backpressure/performance/tracing are actually issues
- Produces useful software, not just examples

---

## Decision Points

**Implement Phase 1.5 if:**
- [ ] Services are dropping messages due to overflow
- [ ] Queue depths are growing unbounded
- [ ] Need to sample/drop messages under load

**Implement Phase 3 if:**
- [ ] Profiling shows `ctx.emit()` as bottleneck (>5% of CPU)
- [ ] Need >10M emits/sec per core
- [ ] Measured latency >120ns is causing issues
- [ ] Ready to open-source and need polished docs

**Implement Phase 4 if:**
- [ ] Have working multi-service Bandit implementation
- [ ] Want to create tutorials for others
- [ ] Need reference implementation for documentation
- [ ] Open-sourcing and need example code

**Implement Phase 2.5 if:**
- [ ] Need distributed tracing across services
- [ ] Integrating with Jaeger/Zipkin
- [ ] Cross-service trace correlation is critical
- [ ] Debugging multi-service issues is painful without it

---

## Current Status: Ready to Build

**What you have (Phases 1-2):**
- ✅ Service API with `#[mycelium::service]` macro
- ✅ ServiceContext with emit(), logging, metrics
- ✅ Automatic observability (trace_id in logs, metrics collection)
- ✅ Actor supervision with exponential backoff
- ✅ Excellent performance (120ns, 8M/sec)
- ✅ Working example demonstrating all features
- ✅ Accurate documentation

**What you can build right now:**
- Single-service applications
- Multi-service applications (same process via Arc)
- Services with complex business logic
- High-throughput data pipelines
- Real-time event processing systems

**What you'll learn by building:**
- If backpressure is a real problem
- If 120ns is fast enough
- If cross-service tracing is needed
- What the actual bottlenecks are
- What features are missing vs. nice-to-have

---

## Conclusion

**Don't build more infrastructure until you know you need it.**

Phases 1-2 provide everything needed to build real applications. The remaining phases are optimizations and polish that should be driven by real requirements, not speculation.

**Next step: Start building Bandit services and discover what you actually need.**
