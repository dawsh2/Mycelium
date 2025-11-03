# Torq v2: Clean Slate Rewrite with Actor Model

**Status**: Design Document
**Created**: 2025-11-02
**Philosophy**: Quality > Quantity. Code is reality. Reality must match spec.

---

## Executive Summary

**Yes, a clean rewrite with actor model from the start is the right approach.**

The current system has accumulated architectural debt that makes it harder to reason about, test, and evolve. Starting fresh with actor model primitives from day one will:

1. **Eliminate coordination complexity** - Location-transparent messaging replaces manual orchestration
2. **Enable true parallel development** - Teams can work on different actors without stepping on each other
3. **Make testing tractable** - Each actor is an isolated unit with clear contracts
4. **Support deployment flexibility** - Same code runs as monolith or distributed cluster
5. **Prevent architectural drift** - GUARDRAILS enforce spec-to-reality consistency from commit #1

**Critical modification to the proposed plan**: The 24-week timeline is optimistic. We'll use **2-week iteration cycles** with **working software at every checkpoint**, using git worktrees for parallel exploration and rigorous PR reviews to maintain quality.

---

## Why Rewrite? (The Honest Assessment)

### Current System Pain Points

1. **Coupling sprawl** - Services share state through Redis in undocumented ways
2. **Testing nightmare** - Integration tests require full stack spinup
3. **Deployment rigidity** - Can't run services independently without breaking assumptions
4. **Performance uncertainty** - No clear bottleneck attribution
5. **Onboarding friction** - New contributors can't understand data flow from code alone

### What We'll Gain

1. **Actor-first architecture** - Message passing as the fundamental primitive
2. **Location transparency** - Same actor code runs in-process or across network
3. **Contract enforcement** - Compile-time guarantees for message types
4. **Testability by design** - Every actor is independently testable
5. **Deployment flexibility** - Config file determines topology (monolith → cluster)

---

## GUARDRAILS Integration (Non-Negotiable)

### Core Principles from `docs/GUARDRAILS/`

1. **Code is reality. Spec mirrors reality. CI enforces sync.**
2. **`system.yaml` is the canonical architecture definition** - Services, events, storage, invariants
3. **Testing pyramid** - 70% unit, 20% integration, 10% E2E (all automated)
4. **Contract-first development** - TLV schemas and service contracts written before code
5. **Financial safety** - If it moves money, test it exhaustively

### New Requirements for Torq v2

#### 1. Pre-Code Architecture Spec

Before writing any service code:

```yaml
# system.yaml (excerpt)
services:
  market_adapter:
    description: "Consumes blockchain events, produces normalized TLVs"
    actor_type: "ReliableConsumer"
    consumes:
      - BlockchainSwapEvent  # External
    produces:
      - PoolStateUpdate      # TLV type 16
      - InstrumentMeta       # TLV type 18
    dependencies:
      - RPC providers (4x rotation)
      - Redis (cache)
    authority:
      - pool_metadata        # Only this actor fetches from RPC
    restart_policy: "one_for_one"
    max_restarts: 3
```

**CI Enforcement**: Service code must implement exactly these contracts. CI fails if:
- Service produces undeclared TLV types
- Service reads from stores it's not authoritative for
- Service communicates in ways not in `system.yaml`

#### 2. Contract-First TLV Development

```rust
// BEFORE implementing any actor, define message contracts

/// Pool state update (TLV type 16, domain: MarketData)
#[derive(TLVMessage, Debug, Clone)]
#[tlv(type = 16, domain = "MarketData", version = 1)]
pub struct PoolStateUpdate {
    pub pool_address: [u8; 20],
    #[tlv(validate = "venue_id_known")]
    pub venue_id: u16,
    pub reserve0: U256,
    pub reserve1: U256,
    pub block_number: u64,
}

// Contract test is written BEFORE actor implementation
#[cfg(test)]
mod contract_tests {
    #[test]
    fn pool_state_serializes_correctly() {
        let update = PoolStateUpdate { /* ... */ };
        let tlv = update.to_tlv();
        assert_eq!(tlv.type_id, 16);

        let roundtrip: PoolStateUpdate = tlv.try_into().unwrap();
        assert_eq!(roundtrip.pool_address, update.pool_address);
    }
}
```

#### 3. Testing Standards (Enforced in CI)

| Test Type | Coverage Target | Max Runtime | Purpose |
|-----------|----------------|-------------|---------|
| Unit | 90% of pure logic | < 1ms each | Validate calculations, parsing |
| Contract | 100% of actor messages | < 100ms each | Verify TLV encode/decode |
| Integration | Actor pairs | < 1s each | Test actor communication |
| Replay | Critical scenarios | < 5s each | Prevent regressions |
| E2E | 3-5 golden paths | < 30s each | Full system validation |

**CI Pipeline**:
```bash
# Pre-commit (local)
cargo test --lib           # Unit tests only (fast)
cargo fmt --check
cargo clippy -- -D warnings

# PR checks (CI)
cargo test                  # All tests except E2E
cargo tarpaulin --out Xml   # Coverage report
python3 scripts/validate_system_yaml.py  # Spec drift check

# Merge to main (CI)
cargo test --ignored        # E2E tests
cargo bench --no-run        # Performance regression check
```

---

## Development Workflow: Git Worktrees + PR Reviews

### Why Worktrees?

**Problem**: Context switching between branches destroys flow state.
**Solution**: Multiple working directories pointing to same git repo.

```bash
# Main development tree
~/torq/main/              # Always on main branch

# Feature worktrees
~/torq/actor-system/      # Building actor framework
~/torq/tlv-codec/         # Implementing TLV codec
~/torq/market-adapter/    # First adapter actor

# Each worktree is independent - no checkout thrashing
```

### Worktree Workflow

#### 1. Create Feature Worktree

```bash
# From main repo
cd ~/torq/main
git worktree add ~/torq/actor-system -b feature/actor-framework

# Now you have two independent directories
cd ~/torq/actor-system
# Work here without disturbing main
```

#### 2. Parallel Development

Developer A works on actor framework:
```bash
cd ~/torq/actor-system
cargo test --lib
git commit -m "Add ActorRef with location transparency"
```

Developer B works on TLV codec simultaneously:
```bash
cd ~/torq/tlv-codec
cargo test --lib
git commit -m "Implement zero-copy TLV deserialization"
```

**No merge conflicts** - they're in separate worktrees!

#### 3. Integration Point

When both features are ready:
```bash
# Merge actor-system first
cd ~/torq/main
git merge feature/actor-framework
cargo test --all  # Ensure no breakage

# Then merge tlv-codec
git merge feature/tlv-codec
cargo test --all
```

#### 4. Cleanup

```bash
git worktree remove ~/torq/actor-system
git branch -d feature/actor-framework
```

### Pull Request Discipline

**Every PR must include:**

1. **System.yaml update** (if architecture changes)
   ```yaml
   # Added in PR #42
   services:
     new_actor:
       consumes: [...]
       produces: [...]
   ```

2. **Contract tests** (100% coverage of new messages)
   ```rust
   #[test]
   fn new_message_roundtrips() { /* ... */ }
   ```

3. **Unit tests** (90%+ coverage of logic)
4. **Documentation** (if public API changes)
5. **Benchmark comparison** (if performance-critical path)

**PR Review Checklist:**

- [ ] Does `system.yaml` reflect these changes?
- [ ] Are all TLV messages tested for roundtrip?
- [ ] Does coverage meet thresholds?
- [ ] Are failure modes documented?
- [ ] Is the commit history clean? (no "fix typo" commits)

**Merge Requirements:**

- ✅ All CI checks pass
- ✅ At least 1 approving review
- ✅ No unresolved discussions
- ✅ Branch is up-to-date with main
- ✅ Squash merge for clean history

---

## Modified Development Chronology

### Key Changes from Original Plan

1. **Shorter iterations** - 2-week sprints instead of 4-week phases
2. **Working software first** - Each sprint delivers runnable artifact
3. **Parallel workstreams** - Use worktrees for concurrent development
4. **Continuous integration** - Merge to main weekly (not at phase end)
5. **Quality gates** - Coverage + performance regressions block merges

### Phase 1: Foundation (Weeks 1-8)

**Sprint 1-2: Core TLV Protocol**

Working artifact: TLV message serialization/deserialization library

```bash
# Deliverable
cargo test --lib -p tlv-codec
# All tests pass, 95%+ coverage

# Can serialize/deserialize messages
let msg = PoolStateUpdate { /* ... */ };
let bytes = msg.to_tlv();
let decoded: PoolStateUpdate = bytes.try_into().unwrap();
```

**Sprint 3-4: Actor Framework**

Working artifact: Location-transparent actor system

```bash
# Deliverable
cargo run --example ping_pong
# Two actors exchange messages via mailbox

# Demonstrates
- Actor spawn/shutdown
- Message passing (local)
- Supervision (restart on panic)
```

**Sprint 5-6: Configuration System**

Working artifact: Configurable deployment modes

```toml
# config/dev.toml
[deployment]
mode = "monolith"
actors = ["*"]

# config/prod.toml
[deployment.market_adapter]
mode = "distributed"
nodes = ["adapter-1:9000", "adapter-2:9000"]
```

```bash
# Deliverable
cargo run --config config/dev.toml    # All actors in-process
cargo run --config config/prod.toml   # Actors distributed
```

**Sprint 7-8: Testing Infrastructure**

Working artifact: Complete test harness

```bash
# Deliverable
cargo test                    # Unit + integration
cargo test --ignored          # E2E tests
cargo tarpaulin --out Html    # Coverage report > 90%
```

### Phase 2: First Trading Strategy (Weeks 9-16)

**Sprint 9-10: Market Data Adapter**

Working artifact: Polygon blockchain → TLV pipeline

```bash
# Deliverable
cargo run --bin market_adapter
# Connects to Polygon WebSocket
# Emits PoolStateUpdate TLVs
# Validates with contract tests
```

**Sprint 11-12: Strategy Framework**

Working artifact: Strategy actor trait + reference implementation

```rust
// Deliverable
pub trait StrategyActor: Actor {
    type Signal: Message;
    async fn on_pool_update(&mut self, update: PoolStateUpdate) -> Option<Self::Signal>;
}

// Reference strategy (trivial)
struct LoggingStrategy;
impl StrategyActor for LoggingStrategy {
    async fn on_pool_update(&mut self, update: PoolStateUpdate) {
        println!("Pool {}: reserves={}/{}",
            hex::encode(update.pool_address),
            update.reserve0,
            update.reserve1);
    }
}
```

**Sprint 13-14: Flash Arbitrage Strategy**

Working artifact: Real arbitrage detector with backtesting

```bash
# Deliverable
cargo run --bin backtest \
    --strategy flash_arb \
    --data tests/replays/2025-10-baseline.json

# Output
Opportunities detected: 47
Profitable (after gas): 3
Total profit: $12.45
```

**Sprint 15-16: Integration + Hardening**

Working artifact: Full pipeline with monitoring

```bash
# Deliverable
docker-compose up
# Starts: Redis, Postgres, market_adapter, strategy, signal_relay
# Dashboard shows: pools tracked, opportunities found, latency p99
```

### Phase 3: Production Readiness (Weeks 17-24)

**Sprint 17-18: Observability**

- Prometheus metrics
- Jaeger tracing
- Grafana dashboards
- Alert rules

**Sprint 19-20: Risk Management**

- Position limits
- Stop-loss mechanisms
- Portfolio tracking
- P&L calculation

**Sprint 21-22: Execution Service**

- Order lifecycle management
- Gas oracle integration
- Transaction signing
- Execution tracking

**Sprint 23-24: Production Deployment**

- Kubernetes manifests
- Load testing (1M msg/s sustained)
- Chaos testing (random pod kills)
- Runbook documentation

---

## Parallel Development Strategy

### Workstream Organization

```
Team A (Foundation):
├─ ~/torq/actor-system      # Core actor framework
├─ ~/torq/tlv-codec         # Message protocol
└─ ~/torq/config-system     # Deployment config

Team B (Domain Logic):
├─ ~/torq/defi-primitives   # AMM math, pool types
├─ ~/torq/market-adapter    # Blockchain ingestion
└─ ~/torq/strategy-framework # Trading strategy trait

Team C (Infrastructure):
├─ ~/torq/observability     # Metrics, tracing
├─ ~/torq/testing-harness   # Test utilities
└─ ~/torq/deployment        # Docker, K8s
```

### Integration Cadence

**Daily**: Push to feature branches, run CI
**Weekly**: Integration sync - merge feature branches to main
**Bi-weekly**: Sprint demo - show working artifact to stakeholders
**Monthly**: Architecture review - validate system.yaml matches reality

---

## Quality Gates (Blocking)

### Pre-Merge Checklist

- [ ] **Coverage**: 90%+ unit, 100% contract tests
- [ ] **Performance**: No p99 latency regressions
- [ ] **Documentation**: All public APIs documented
- [ ] **System.yaml**: Updated if architecture changed
- [ ] **Benchmarks**: No unexpected memory/CPU increases
- [ ] **Replays**: All golden tests pass
- [ ] **Linting**: `cargo clippy` with zero warnings

### Monthly Audit

- **Dependency audit**: `cargo audit` - no critical CVEs
- **Dead code**: `cargo-udeps` - remove unused deps
- **Binary size**: Track total binary size (should not balloon)
- **Technical debt**: Review FIXME/TODO comments, schedule cleanup

---

## Risk Mitigation

### Technical Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|-----------|
| Actor framework bugs | Medium | High | Comprehensive unit + integration tests |
| TLV protocol evolution | Low | Medium | Versioned messages, backward compat |
| Performance bottlenecks | Medium | High | Continuous benchmarking, profiling |
| State explosion (actor memory) | Low | Medium | Supervision policies, monitoring |

### Process Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|-----------|
| Feature creep | High | High | Strict sprint scope, 2-week lockdown |
| Technical debt accumulation | Medium | High | Monthly cleanup sprints |
| Spec drift | Medium | High | CI enforcement via system.yaml validation |
| Team burnout | Low | Critical | Quality > speed, sustainable pace |

---

## Success Metrics

### Technical KPIs

- **Latency**: p50 < 50ns (local), p99 < 100μs (IPC), p999 < 5ms (network)
- **Throughput**: 1M msg/s sustained (local), 500K msg/s (IPC)
- **Uptime**: 99.95%+ (excluding planned maintenance)
- **Test coverage**: 90%+ overall, 100% critical paths
- **Build time**: < 5min clean build, < 30sec incremental
- **Backtest speed**: 10x+ parallel speedup vs sequential

### Business KPIs

- **Strategy profitability**: Positive P&L in 3-month backtest
- **Execution quality**: < 0.5% average slippage
- **Risk compliance**: Zero position limit breaches
- **Development velocity**: Working artifact every 2 weeks

---

## Anti-Patterns to Avoid

### 1. Big Bang Integration

**Wrong**:
```
Week 1-12: Build everything in isolation
Week 13: Try to integrate for first time
Week 14-20: Debug integration hell
```

**Right**:
```
Week 1-2: TLV codec (works standalone)
Week 3: Integrate with actor framework
Week 4: Deploy to staging, fix issues
Week 5: Repeat for next component
```

### 2. Premature Optimization

**Wrong**:
```rust
// First implementation - way too complex
struct ZeroCopyCircularBufferPool<T, const N: usize> { /* 500 lines */ }
```

**Right**:
```rust
// First implementation - simple
struct Mailbox<T> {
    queue: VecDeque<T>,
}

// Later, after profiling shows it's a bottleneck
struct OptimizedMailbox<T> { /* ... */ }
```

### 3. Undocumented Clever Code

**Wrong**:
```rust
// No comment, uses unsafe, unclear purpose
unsafe { std::mem::transmute::<&[u8], &Header>(bytes.as_ptr()) }
```

**Right**:
```rust
/// Zero-copy parse header from byte slice.
/// SAFETY: Header is repr(C) with no padding, slice is validated to be 32 bytes.
let header = unsafe {
    zerocopy::Ref::<_, Header>::new(bytes)
        .expect("Header must be 32 bytes aligned")
        .into_ref()
};
```

### 4. "We'll Add Tests Later"

**Wrong**: Ship feature, promise to add tests in next sprint (never happens)

**Right**: Test-driven development
1. Write contract test (defines expected behavior)
2. Implement feature (makes test pass)
3. PR includes both feature + tests

---

## Migration Strategy (From Current System)

### Parallel Run Phase (Weeks 17-24)

Run Torq v1 and v2 side-by-side in production:

1. **Shadow mode** - v2 consumes same events as v1, compares outputs
2. **Divergence alerts** - Log when v1 and v2 disagree on signals
3. **Gradual cutover** - Route 1% → 10% → 50% → 100% traffic to v2
4. **Rollback plan** - Feature flag to revert to v1 instantly

### Data Migration

```sql
-- Postgres migration: Add v2 schema
CREATE SCHEMA torq_v2;

-- Copy data, transform if needed
INSERT INTO torq_v2.pool_metadata
SELECT
    pool_address,
    token0_address,
    token1_address,
    -- v2 requires non-null decimals
    COALESCE(token0_decimals, 18) as token0_decimals,
    -- ...
FROM torq_v1.pools;
```

### Sunset v1 (Week 25+)

After 4 weeks of stable v2 in production:
1. Stop v1 services
2. Archive v1 codebase to `torq-v1-archive/`
3. Redirect all traffic to v2
4. Decommission v1 infrastructure

---

## Appendices

### A. System.yaml Example (Full Service Definition)

```yaml
services:
  market_adapter:
    description: "Polygon blockchain → TLV pipeline"
    actor_type: "ReliableConsumer"
    consumes:
      - source: websocket
        format: "JSON-RPC 2.0 notification"
        events: ["eth_subscription (newPendingTransactions, logs)"]
    produces:
      - tlv_type: 16  # PoolStateUpdate
        domain: MarketData
        ordering: "per-pool sequential, cross-pool concurrent"
      - tlv_type: 18  # InstrumentMeta
        domain: MarketData
        ordering: "at-most-once per token"
    dependencies:
      rpc_providers:
        - "https://polygon-rpc.com"
        - "https://polygon-mainnet.g.alchemy.com/v2/..."
      redis:
        keys: ["pool:{address}", "token:{address}"]
        authority: read_write
      postgres:
        tables: ["pool_metadata", "token_metadata"]
        authority: write_only
    restart_policy:
      strategy: "one_for_one"
      max_restarts: 3
      window_secs: 60
    invariants:
      - "InstrumentMeta emitted before first PoolStateUpdate for any pool"
      - "Pool metadata cached in Redis before RPC fetch"
```

### B. Git Worktree Cheat Sheet

```bash
# List all worktrees
git worktree list

# Create new worktree + branch
git worktree add ~/torq/feature-name -b feature/feature-name

# Create worktree from existing branch
git worktree add ~/torq/bugfix origin/bugfix/critical-issue

# Remove worktree (keep branch)
git worktree remove ~/torq/feature-name

# Remove worktree (delete branch)
git worktree remove ~/torq/feature-name
git branch -D feature/feature-name

# Prune stale worktrees
git worktree prune
```

### C. Pre-Commit Hook Template

```bash
#!/bin/bash
# .git/hooks/pre-commit

set -e

echo "Running pre-commit checks..."

# 1. Format check
cargo fmt -- --check || {
    echo "❌ Code not formatted. Run: cargo fmt"
    exit 1
}

# 2. Clippy
cargo clippy -- -D warnings || {
    echo "❌ Clippy warnings found"
    exit 1
}

# 3. Unit tests
cargo test --lib || {
    echo "❌ Unit tests failed"
    exit 1
}

# 4. System.yaml validation
python3 scripts/validate_system_yaml.py || {
    echo "❌ System.yaml validation failed"
    exit 1
}

echo "✅ All pre-commit checks passed"
```

---

## Conclusion

**Rewriting Torq with actor model from day one is the right call.**

The proposed 24-week timeline is achievable **if and only if**:

1. We use **2-week sprints** with working artifacts every cycle
2. We leverage **git worktrees** for true parallel development
3. We enforce **GUARDRAILS** (system.yaml, contract tests, CI gates)
4. We prioritize **quality over speed** (no shortcuts on tests/docs)
5. We do **continuous integration** (merge weekly, not at phase end)

The alternative (incremental refactor of current system) is slower and riskier. Clean slate means:
- No legacy assumptions to work around
- Clear actor boundaries from commit #1
- Testing built into DNA (not bolted on later)
- Deployment flexibility baked in (not retrofitted)

**Recommendation**: Approve rewrite, allocate 6 months, start with Phase 1 Sprint 1 (TLV protocol).

First PR should be: `system.yaml` defining the complete architecture. Code follows spec, not the other way around.

---

**Quality > Quantity. Code is reality. Reality must match spec. CI enforces truth.**
