# Mycelium: Clean Rewrite with Transport Abstraction

**Status**: Foundation Complete (v0.1.0)
**Created**: 2025-11-02  
**Philosophy**: Quality > Quantity. Code is reality. Reality must match spec.

---

## Executive Summary

**Yes, a clean rewrite with transport abstraction from the start is the right approach.**

The current Torq system has accumulated architectural debt that makes it harder to reason about, test, and evolve. Starting fresh with **pub/sub messaging** and **adaptive transport** from day one will:

1. **Eliminate deployment complexity** - Same code runs as monolith or distributed
2. **Enable true parallel development** - Teams work on different services without coordination
3. **Make testing tractable** - Each service is independently testable
4. **Support deployment flexibility** - Config file determines topology
5. **Prevent architectural drift** - GUARDRAILS enforce spec-to-reality consistency

**Timeline**: **8 weeks** using 2-week iteration cycles with working software at every checkpoint.

---

## Why Rewrite? (The Honest Assessment)

### Current Torq Pain Points

1. **Coupling sprawl** - Services share state through Redis in undocumented ways
2. **Testing nightmare** - Integration tests require full stack spinup
3. **Deployment rigidity** - Can't run services independently
4. **Performance uncertainty** - No clear bottleneck attribution
5. **Onboarding friction** - New contributors can't understand data flow from code

### What We'll Gain with Mycelium

1. **Pub/sub messaging** - Type-safe event streaming as the fundamental primitive
2. **Adaptive transport** - Arc<T> (local) → Unix sockets (IPC) → TCP (distributed)
3. **Contract enforcement** - Compile-time guarantees for message types
4. **Testability by design** - Every service is independently testable
5. **Deployment flexibility** - Config file determines topology (monolith → cluster)

---

## Architecture: Pub/Sub with Adaptive Transport

### Core Principle

**Same pub/sub code, different performance based on deployment topology.**

```rust
// Developer writes this once:
let pub_ = bus.publisher::<SwapEvent>();
pub_.publish(event).await?;

// Runtime chooses transport:
// - Monolith:      Arc<T> clone         (~200ns, zero-copy)
// - Multi-process: Unix domain socket   (~50μs, zero-copy via shared mem)
// - Distributed:   TCP with rkyv        (~500μs, zero-copy deserialization)
```

**Not an actor framework**. Not point-to-point messaging. Just **simple pub/sub** with smart routing.

---

## What We're Building

### Core Components (✅ Complete)

1. **Message Protocol** (`mycelium-protocol`)
   - Type-safe messages with TYPE_ID and TOPIC
   - Envelope abstraction for transport
   - rkyv serialization for zero-copy remote messaging

2. **Transport Layer** (`mycelium-transport`)
   - Local transport (Arc<T> pub/sub)
   - Unix socket transport (IPC)
   - TCP transport (distributed)
   - TLV wire protocol
   - Generic stream handling

3. **Configuration System** (`mycelium-config`)
   - Topology-driven deployment
   - DeploymentMode (Monolith, Bundled, Distributed)
   - TOML-based configuration

4. **MessageBus** - Unified API
   - Automatic transport selection
   - Service discovery via topology
   - Lazy connection management

### What We're NOT Building

- ❌ Actor supervision trees (use systemd/k8s)
- ❌ Request/reply patterns (use separate topics)
- ❌ Distributed consensus (use external coordination)
- ❌ Custom process management (use systemd/k8s)

**Keep it simple**. Add complexity only when proven necessary.

---

## GUARDRAILS Integration (Non-Negotiable)

### Core Principles from `docs/GUARDRAILS/`

1. **Code is reality. Spec mirrors reality. CI enforces sync.**
2. **`system.yaml` is the canonical architecture definition**
3. **Testing pyramid** - 70% unit, 20% integration, 10% E2E
4. **Contract-first development** - Message schemas before code
5. **Financial safety** - If it moves money, test it exhaustively

### Requirements for Mycelium

#### 1. Pre-Code Architecture Spec

Before writing any service code:

```yaml
# system.yaml (excerpt)
services:
  polygon_adapter:
    description: "Consumes Polygon events, produces normalized SwapEvents"
    bundle: "adapters"
    publishes:
      - SwapEvent        # TYPE_ID 100
      - PoolMetadata     # TYPE_ID 101
    subscribes: []
    dependencies:
      - Polygon RPC (4x rotation)
      - Redis (cache)
    restart_policy: "systemd"
```

**CI Enforcement**: Service code must implement exactly these contracts.

#### 2. Contract-First Message Development

```rust
// BEFORE implementing any service, define message contracts

#[derive(Debug, Clone, Archive, Serialize, Deserialize)]
#[archive(check_bytes)]
pub struct SwapEvent {
    pub pool: Address,
    pub token_in: Address,
    pub amount_in: U256,
    pub amount_out: U256,
}

impl_message!(SwapEvent, 100, "market-data");

// Contract test is written BEFORE service implementation
#[cfg(test)]
mod contract_tests {
    #[test]
    fn swap_event_roundtrip() {
        let event = SwapEvent { /* ... */ };
        let bytes = rkyv::to_bytes(&event).unwrap();
        let decoded: SwapEvent = rkyv::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.pool, event.pool);
    }
}
```

#### 3. Testing Standards (Enforced in CI)

| Test Type | Coverage Target | Max Runtime | Purpose |
|-----------|----------------|-------------|---------|
| Unit | 90% of pure logic | < 1ms each | Validate calculations |
| Contract | 100% of messages | < 100ms each | Verify serialization |
| Integration | Service pairs | < 1s each | Test pub/sub |
| E2E | 3-5 golden paths | < 30s each | Full pipeline |

**CI Pipeline**:
```bash
# Pre-commit (local)
cargo test --lib           # Unit tests only (fast)
cargo fmt --check
cargo clippy -- -D warnings

# PR checks (CI)
cargo test                  # All tests
cargo tarpaulin --out Xml   # Coverage report
python3 scripts/validate_system_yaml.py

# Merge to main (CI)
cargo test --ignored        # E2E tests
cargo bench --no-run        # Performance check
```

---

## Development Workflow: Git Worktrees + PR Reviews

### Why Worktrees?

**Problem**: Context switching destroys flow state.
**Solution**: Multiple working directories for same repo.

```bash
# Main development tree
~/mycelium/main/              # Always on main branch

# Feature worktrees
~/mycelium/polygon-adapter/   # Building first adapter
~/mycelium/strategy/          # Building first strategy
~/mycelium/observability/     # Adding metrics

# Each worktree is independent - no checkout thrashing
```

### Worktree Workflow

```bash
# Create feature worktree
git worktree add ~/mycelium/polygon-adapter -b feature/polygon-adapter

# Work in isolation
cd ~/mycelium/polygon-adapter
cargo test
git commit -m "Add Polygon adapter"

# Merge when ready
cd ~/mycelium/main
git merge feature/polygon-adapter

# Cleanup
git worktree remove ~/mycelium/polygon-adapter
```

### Pull Request Discipline

**Every PR must include:**

1. **System.yaml update** (if architecture changes)
2. **Contract tests** (100% coverage of new messages)
3. **Unit tests** (90%+ coverage)
4. **Documentation** (if public API changes)

**Merge Requirements:**

- ✅ All CI checks pass
- ✅ At least 1 approving review
- ✅ Branch up-to-date with main
- ✅ Squash merge for clean history

---

## Development Chronology (8 Weeks)

### Phase 1: Foundation (Weeks 1-2) ✅ COMPLETE

**Sprint 1: Core Transport Protocol**

Deliverable: Working transport layer with Arc, Unix, TCP

```bash
# ✅ Completed
cargo test -p mycelium-protocol -p mycelium-config -p mycelium-transport
# Result: 52 tests passed

# Can publish/subscribe across transports
let pub_ = bus.publisher::<SwapEvent>();
pub_.publish(event).await?;
```

**What was built**:
- ✅ Message protocol (TYPE_ID, TOPIC, Message trait)
- ✅ Local transport (Arc-based pub/sub)
- ✅ Unix socket transport (IPC)
- ✅ TCP transport (distributed)
- ✅ TLV wire protocol
- ✅ rkyv serialization
- ✅ MessageBus with automatic transport selection
- ✅ Topology configuration (TOML)
- ✅ Comprehensive test suite
- ✅ Code refactoring (eliminated 220 lines of duplication)

**Status**: Foundation is rock-solid. Ready for services.

---

### Phase 2: First Service (Weeks 3-4) 🚧 NEXT

**Sprint 2: Polygon Market Data Adapter**

Deliverable: Real adapter publishing swap events

```bash
# Goal
cargo run --bin polygon-adapter --config config/dev.toml
# Connects to Polygon RPC
# Publishes SwapEvent messages to Mycelium transport
```

**Tasks**:
1. Create `crates/services/polygon-adapter`
2. Integrate with Polygon RPC (ethers-rs or alloy)
3. Parse swap logs → SwapEvent
4. Publish via MessageBus
5. Add health checks
6. Add metrics (events/sec, latency)

**Success criteria**:
- Publishes real swap events from Polygon mainnet
- 100% uptime for 24h continuous run
- < 1s latency from blockchain to publish

---

### Phase 3: First Strategy (Weeks 5-6)

**Sprint 3: Flash Arbitrage Detector**

Deliverable: Strategy that consumes SwapEvents

```bash
# Goal
cargo run --bin flash-arbitrage --config config/dev.toml
# Subscribes to SwapEvent
# Detects arbitrage opportunities
# Publishes ArbitrageSignal
```

**Tasks**:
1. Create `crates/services/flash-arbitrage`
2. Subscribe to SwapEvent via MessageBus
3. Implement arbitrage detection logic
4. Publish ArbitrageSignal
5. Add backtesting support
6. Validate with historical data

**Success criteria**:
- Correctly identifies profitable opportunities in backtests
- < 100μs processing latency per event
- Zero false positives in test suite

---

### Phase 4: Production Readiness (Weeks 7-8)

**Sprint 4: Observability + Hardening**

Deliverable: Production-ready deployment

**Tasks**:

1. **Observability**
   - Integrate `tracing` crate
   - Prometheus metrics export
   - Grafana dashboards
   - Alert rules (latency, errors, restarts)

2. **Error Handling**
   - Graceful degradation patterns
   - Circuit breakers for external APIs
   - Retry policies (exponential backoff)

3. **Deployment**
   - Docker images for each service
   - Kubernetes manifests
   - Helm charts
   - Runbook documentation

4. **Testing**
   - Load testing (1M msg/sec sustained)
   - Chaos testing (kill pods randomly)
   - Network partition testing

**Success criteria**:
- 99.9%+ uptime in staging
- p99 latency < 5ms end-to-end
- Clean failover when services crash

---

## Parallel Development Strategy

### Workstream Organization

```
Team A (Services):
├─ ~/mycelium/polygon-adapter    # Market data ingestion
├─ ~/mycelium/flash-arbitrage    # Strategy implementation
└─ ~/mycelium/order-executor     # Trade execution

Team B (Infrastructure):
├─ ~/mycelium/observability      # Metrics, tracing, dashboards
├─ ~/mycelium/testing-harness    # Load testing, chaos testing
└─ ~/mycelium/deployment         # Docker, K8s, Helm
```

### Integration Cadence

- **Daily**: Push to feature branches, run CI
- **Weekly**: Integration sync - merge to main
- **Bi-weekly**: Sprint demo - show working artifact
- **End of 8 weeks**: Production deployment

---

## Quality Gates (Blocking)

### Pre-Merge Checklist

- [ ] **Coverage**: 90%+ unit, 100% contract tests
- [ ] **Performance**: No p99 latency regressions
- [ ] **Documentation**: All public APIs documented
- [ ] **System.yaml**: Updated if architecture changed
- [ ] **Linting**: `cargo clippy` with zero warnings

### Monthly Audit

- **Dependency audit**: `cargo audit` - no critical CVEs
- **Dead code**: `cargo-udeps` - remove unused deps
- **Binary size**: Track total binary size
- **Technical debt**: Review FIXME/TODO, schedule cleanup

---

## Risk Mitigation

### Technical Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|-----------|
| Transport layer bugs | Low | High | ✅ Complete test suite (52 tests) |
| Message protocol evolution | Low | Medium | Versioned messages, backward compat |
| Performance bottlenecks | Medium | High | Continuous benchmarking |
| External API failures | High | Medium | Circuit breakers, retry policies |

### Process Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|-----------|
| Feature creep | High | High | Strict 8-week timeline |
| Technical debt | Medium | High | Monthly cleanup sprints |
| Spec drift | Medium | High | CI enforcement via system.yaml |
| Team burnout | Low | Critical | Sustainable pace, quality > speed |

---

## Success Metrics

### Technical KPIs

- **Latency**: p50 < 50ns (local), p99 < 100μs (IPC), p999 < 5ms (network)
- **Throughput**: 1M msg/sec sustained (local), 200K msg/sec (IPC)
- **Uptime**: 99.9%+ (excluding planned maintenance)
- **Test coverage**: 90%+ overall, 100% critical paths
- **Build time**: < 5min clean build, < 30sec incremental

### Business KPIs

- **Strategy profitability**: Positive P&L in 3-month backtest
- **Execution quality**: < 0.5% average slippage
- **Development velocity**: Working artifact every 2 weeks

---

## Anti-Patterns to Avoid

### 1. Big Bang Integration

**Wrong**: Build everything for 6 weeks, integrate at the end.
**Right**: Integrate continuously. Deploy to staging weekly.

### 2. Premature Optimization

**Wrong**: Over-engineer from day one.
**Right**: Start simple. Profile. Optimize bottlenecks.

### 3. Undocumented Code

**Wrong**: Clever code with no explanation.
**Right**: Clear code with comments for non-obvious parts.

### 4. "We'll Add Tests Later"

**Wrong**: Ship feature, promise tests in next sprint (never happens).
**Right**: Test-driven development. Tests in same PR as feature.

---

## Migration from Torq

### Parallel Run Phase (Weeks 7-8)

Run Torq and Mycelium side-by-side:

1. **Shadow mode** - Mycelium consumes same events, compares outputs
2. **Divergence alerts** - Log when Torq and Mycelium disagree
3. **Gradual cutover** - Route 1% → 10% → 50% → 100% traffic
4. **Rollback plan** - Feature flag to revert instantly

### Data Migration

```sql
-- Postgres migration: Add mycelium schema
CREATE SCHEMA mycelium;

-- Copy data, transform if needed
INSERT INTO mycelium.pool_metadata
SELECT pool_address, token0, token1, ...
FROM torq.pools;
```

### Sunset Torq (Week 9+)

After 2 weeks stable in production:
1. Stop Torq services
2. Archive Torq codebase
3. Redirect all traffic to Mycelium
4. Decommission Torq infrastructure

---

## Current Status (Week 2 Complete)

### ✅ Completed

- [x] Core message protocol
- [x] Local transport (Arc)
- [x] Unix socket transport
- [x] TCP transport
- [x] TLV wire protocol
- [x] rkyv serialization
- [x] MessageBus API
- [x] Topology configuration
- [x] 52 comprehensive tests
- [x] Code refactoring (90% reduction in duplication)
- [x] Documentation (TRANSPORT.md)

### 🚧 Next Sprint (Weeks 3-4)

- [ ] Polygon adapter service
- [ ] Health check framework
- [ ] Metrics export (Prometheus)
- [ ] Integration with real blockchain data

### 📋 Upcoming (Weeks 5-8)

- [ ] Flash arbitrage strategy
- [ ] Backtesting framework
- [ ] Order executor service
- [ ] Full observability stack
- [ ] Production deployment

---

## Appendices

### A. System.yaml Example

```yaml
services:
  polygon_adapter:
    description: "Polygon blockchain → SwapEvent pipeline"
    bundle: "adapters"
    publishes:
      - type: SwapEvent
        type_id: 100
        topic: "market-data"
    subscribes: []
    dependencies:
      - Polygon RPC
      - Redis (pool metadata cache)
    restart_policy: "systemd one-for-one"
```

### B. Git Worktree Cheat Sheet

```bash
# List worktrees
git worktree list

# Create worktree
git worktree add ~/mycelium/feature-name -b feature/feature-name

# Remove worktree
git worktree remove ~/mycelium/feature-name
git branch -D feature/feature-name
```

### C. Pre-Commit Hook

```bash
#!/bin/bash
set -e

cargo fmt -- --check
cargo clippy -- -D warnings
cargo test --lib
python3 scripts/validate_system_yaml.py

echo "✅ All checks passed"
```

---

## Conclusion

**Phase 1 (Foundation) is complete.** The transport layer is solid, tested, and ready for services.

**8-week timeline is achievable** if we:

1. Use **2-week sprints** with working artifacts
2. Leverage **git worktrees** for parallel development
3. Enforce **GUARDRAILS** (system.yaml, tests, CI)
4. Prioritize **quality over speed**
5. Do **continuous integration** (merge weekly)

**Recommendation**: Proceed to Phase 2 (Polygon adapter). First service will validate all our design decisions.

First task: Define `SwapEvent` message contract in `system.yaml`. Code follows spec, not the other way around.

---

**Quality > Quantity. Code is reality. Reality must match spec. CI enforces truth.** 🦀
