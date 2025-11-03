# Architecture Guardrails

## Philosophy

- **Code is reality.**
- **The architecture spec mirrors reality.**
- **Diagrams and docs are generated from those two.**
- **CI enforces sync.** Nothing merges if reality and description disagree.

If someone forgets, the build fails. If someone tries to freeload undocumented behavior into prod, the build fails.

---

## Current State: Greenfield Foundation

We're building Mycelium from scratch, which gives us a unique advantage: we can establish guardrails **from commit #1**.

Unlike the Torq rewrite where we're retrofitting documentation, here we:
1. **Define the architecture first** (system.yaml)
2. **Generate diagrams from the spec**
3. **Write contract tests before implementation**
4. **Build code that matches the spec**

**No drift. No archaeology. Just clean architecture from day one.**

---

## Evolution Path

### Phase 1: Spec-First Development (NOW)
**Goal**: Define canonical architecture before writing code

- Create `system.yaml` defining:
  - Actor types and responsibilities
  - Message protocols (TLV types)
  - Transport mechanisms (local/unix/tcp)
  - Deployment topologies
- Generate Mermaid diagrams from spec
- Write contract tests for message formats
- Implement actors to match contracts

**Anti-rot mechanism**: CI validates code matches spec before merge

### Phase 2: Contract Testing (NEXT)
**Goal**: Every message type has 100% test coverage

- Message roundtrip tests (serialize → deserialize)
- Actor interface contracts
- Transport layer contracts
- No actor communicates in undocumented ways

**Anti-rot mechanism**: Coverage gates in CI

### Phase 3: Runtime Validation (FUTURE)
**Goal**: Production validates architectural invariants

- Actor supervision ensures fault isolation
- Message ordering constraints enforced at runtime
- Deployment topology validates at startup
- Observability captures contract violations

**Anti-rot mechanism**: Production rejections alert on-call

---

## Directory Structure

```
docs/GUARDRAILS/
├── README.md                    # This file
├── system.yaml                  # ⭐ Canonical architecture definition
├── config-vs-spec.md           # Relationship between system.yaml and config/*.toml
├── testing-standards.md        # Comprehensive testing framework
├── diagrams/                    # Generated from system.yaml
│   ├── actor-topology.mmd      # Actor hierarchy and supervision
│   ├── message-flow.mmd        # Message passing between actors
│   └── deployment-modes.mmd    # Monolith vs distributed topologies
├── specs/                       # Data specifications
│   ├── messages.md             # TLV message type specifications
│   ├── actor-contracts.md      # Actor interface contracts
│   └── deployment.md           # Deployment topology spec
├── generator/                   # ⭐ Diagram generation tools
│   ├── generate_diagrams.py    # Generates diagrams from system.yaml
│   └── validate_system.py      # CI validation
└── MIGRATIONS/                  # Architecture migration tracking
    ├── README.md               # Migration workflow
    └── templates/
        └── migration-template.md
```

---

## The `system.yaml` - Canonical Architecture Definition

### What is `system.yaml`?

`system.yaml` is the **canonical definition** of the Mycelium architecture. It specifies:
- **Actors** (what messages they consume/produce, their supervision strategy)
- **Messages** (TLV types, domains, schema versions, ordering invariants)
- **Transports** (local Arc, Unix sockets, TCP, adaptive selection)
- **Deployment modes** (monolith, multi-process, distributed)
- **Invariants** (message ordering, fault isolation, backpressure)

### How is it different from `config/*.toml`?

**Critical distinction**:
- **`system.yaml`** = What the system IS (architecture spec)
- **`config/*.toml`** = How the system RUNS (runtime settings)

See [config-vs-spec.md](./config-vs-spec.md) for detailed comparison.

### Example Structure

```yaml
actors:
  polygon_adapter:
    description: "Polygon blockchain data ingestion"
    type: adapter
    consumes:
      - BlockchainEvent  # External
    produces:
      - SwapEvent        # TLV type 11, domain MarketData
      - PoolDiscovered   # TLV type 12, domain MarketData
    supervision: restart_on_failure
    dependencies:
      - polygon_websocket

  flash_arbitrage:
    description: "Flash arbitrage opportunity detection"
    type: strategy
    consumes:
      - SwapEvent        # From adapters
      - PoolDiscovered   # From adapters
    produces:
      - ArbitrageSignal  # TLV type 20, domain Signal
    supervision: restart_with_backoff
    dependencies: []

messages:
  SwapEvent:
    tlv_type: 11
    domain: MarketData
    schema_version: v1
    description: "DEX swap event with pool state update"
    fields:
      - pool_address: "[u8; 20]"
      - amount_in: "U256"
      - amount_out: "U256"
      - reserves: "[U256; 2]"
      - block_number: "u64"
    invariants:
      - "block_number must be monotonically increasing per pool"

  ArbitrageSignal:
    tlv_type: 20
    domain: Signal
    schema_version: v1
    description: "Profitable arbitrage opportunity"
    sensitivity: financially_destructive_if_leaked
    log_payload: false
    fields:
      - opportunity_id: "u64"
      - path: "Vec<[u8; 20]>"
      - estimated_profit: "U256"

transports:
  local:
    description: "Arc<T> for actors in same process"
    latency_us: "<1"

  unix_socket:
    description: "Unix domain sockets for local IPC"
    latency_us: "10-50"

  tcp:
    description: "TCP for distributed deployment"
    latency_us: "100-1000"

deployment_modes:
  development:
    topology: monolith
    transport: local
    actors: [all]

  production:
    topology: distributed
    transport: adaptive  # local > unix > tcp
    bundles:
      - name: market_data
        actors: [polygon_adapter, ethereum_adapter]
      - name: strategies
        actors: [flash_arbitrage]
```

### Using the Diagram Generator

```bash
# Generate all diagrams from system.yaml
cd docs/GUARDRAILS/generator
python3 generate_diagrams.py

# Validate generated diagrams haven't drifted
python3 generate_diagrams.py --validate
```

**CI Integration**:
```bash
# In .github/workflows/ci.yml
- name: Validate Architecture
  run: |
    cd docs/GUARDRAILS/generator
    python3 validate_system.py || exit 1
```

---

## Why Spec-First for Mycelium?

**Advantages of starting with guardrails:**

1. **No Technical Debt**: We define the right architecture upfront
2. **Clear Contracts**: Actors know exactly what messages to expect
3. **Parallel Development**: Teams can work on different actors independently
4. **Testability**: Contract tests written before implementation
5. **Documentation**: Always up-to-date because it's the source of truth

**Contrast with Torq**:
- Torq: Code exists → document it → enforce sync
- Mycelium: Spec exists → implement code → validate match

---

## Documentation Standards

### Actor Specifications Must Include:

1. **Type**: Adapter, Strategy, Execution, Infrastructure
2. **Consumes**: Input message types
3. **Produces**: Output message types
4. **Supervision**: How the runtime handles failures
5. **Dependencies**: External systems (RPC, Redis, etc.)
6. **Invariants**: What it guarantees
7. **Failure Modes**: How it handles errors

### Message Specifications Must Include:

1. **TLV Type**: Unique identifier (domain + type)
2. **Schema Version**: For evolution
3. **Fields**: Complete type specification
4. **Invariants**: Ordering, validation rules
5. **Producers**: Which actors can emit this
6. **Consumers**: Which actors process this
7. **Sensitivity**: Whether to log payload

### Example Message Spec

```markdown
## SwapEvent (TLV Type 11, MarketData Domain)

**Description**: Real-time DEX swap with reserve update

**Schema Version**: v1

**Fields**:
- `pool_address`: [u8; 20] - DEX pool address
- `amount_in`: U256 - Input token amount
- `amount_out`: U256 - Output token amount
- `reserves`: [U256; 2] - Post-swap reserves [token0, token1]
- `block_number`: u64 - Block height
- `timestamp`: u64 - Unix timestamp (seconds)

**Invariants**:
- `block_number` must be monotonically increasing per pool
- `reserves` must both be > 0
- `timestamp` must be within 60s of system time

**Producers**: [polygon_adapter, ethereum_adapter]
**Consumers**: [flash_arbitrage, market_maker]

**Sensitivity**: Public (safe to log)

**Contract Test**: `tests/contract/swap_event_roundtrip.rs`
```

---

## Contract Testing

Every message type MUST have a contract test:

```rust
// tests/contract/swap_event_roundtrip.rs

#[test]
fn swap_event_roundtrip() {
    let original = SwapEvent {
        pool_address: [0x12; 20],
        amount_in: U256::from(1000),
        amount_out: U256::from(990),
        reserves: [U256::from(100_000), U256::from(99_000)],
        block_number: 12345678,
        timestamp: 1699234567,
    };

    // Encode to TLV
    let encoded = original.encode_tlv();

    // Decode back
    let decoded = SwapEvent::decode_tlv(&encoded).unwrap();

    // Must roundtrip exactly
    assert_eq!(original, decoded);
}

#[test]
fn swap_event_rejects_zero_reserves() {
    let invalid = SwapEvent {
        reserves: [U256::ZERO, U256::from(100_000)],
        ..default()
    };

    // Must fail validation
    assert!(invalid.validate().is_err());
}

#[test]
fn swap_event_rejects_old_timestamps() {
    let old = SwapEvent {
        timestamp: SystemTime::now() - Duration::from_secs(120),
        ..default()
    };

    // Must fail validation (>60s old)
    assert!(old.validate().is_err());
}
```

**Coverage requirement**: 100% for all message types.

---

## Architectural Invariants

Mycelium enforces these invariants:

### Runtime Invariants
- **Fault Isolation**: Actor crash cannot crash supervisor
- **Mailbox Bounded**: Actors apply backpressure when overwhelmed
- **Supervision Strategy**: Defined per-actor in system.yaml
- **No Shared Mutable State**: Actors communicate only via messages

### Message Invariants
- **Schema Versioning**: All messages have version field
- **Idempotency**: Duplicate messages are safe to process
- **Ordering**: Defined per-message-type in system.yaml
- **Validation**: All messages validated before processing

### Deployment Invariants
- **Transport Selection**: Adaptive based on topology
- **Bundle Isolation**: Actors in different bundles can't use Arc<T>
- **Discovery**: Actors find each other via registry, not hardcoded

---

## Implemented Guardrails (Phase 1)

### 1. Code Duplication Detection ✅ ACTIVE

**Purpose**: Prevent LLM-assisted development from introducing duplicate code patterns

**Location**: `scripts/detect_duplication/`

**How it works**:
- AST-based analysis using `syn` crate
- Compares all functions pairwise for similarity
- Semantic matching (not just textual)
- Threshold: 70% similarity = CI failure

**Usage**:
```bash
# Run locally
cd scripts/detect_duplication
cargo build --release
./target/release/detect_duplication --path ../../crates

# Runs automatically in CI on every PR
```

**What it catches**:
- Copy-pasted functions with minimal changes
- Similar logic that should be extracted to shared function
- Repeated patterns that need trait abstraction

**CI Integration**: `.github/workflows/ci.yml` - `duplication` job

### 2. Comprehensive Test Suite ✅ ACTIVE

**Coverage requirements**:
- Unit tests: 70% coverage minimum
- Contract tests: 100% for all message types
- Doc tests: All examples must compile

**CI Integration**: `.github/workflows/ci.yml` - `test` job

### 3. Clippy Lints ✅ ACTIVE

**Standards enforced**:
- `RUSTFLAGS="-D warnings"` - All warnings are errors
- Clippy with standard lints
- Format checking with `cargo fmt`

**CI Integration**: `.github/workflows/ci.yml` - `clippy` and `fmt` jobs

### 4. Documentation Tests ✅ ACTIVE

**Requirements**:
- All public APIs documented
- Code examples in docs must compile
- `cargo test --doc` passes

**CI Integration**: Part of `test` job

---

## Planned Guardrails (Future)

### system.yaml Validation (Deferred)

**Status**: Intentionally deferred - waiting for 3+ services before adding

**Rationale**: With only transport layer + protocol, system.yaml would be over-engineering. Will add when:
- We have 3+ services with dependencies
- Service boundaries become unclear
- New contributors get confused about architecture

**When to implement**: After Polygon adapter + flash arbitrage strategy

---

## Success Metrics

We'll know the guardrails are working when:

1. **Zero Architecture Drift**: system.yaml always matches code
2. **100% Contract Coverage**: Every message type has tests
3. **Fast Onboarding**: New contributors understand system from diagrams
4. **Confident Refactoring**: Change internals without breaking contracts
5. **Production Validation**: Runtime rejects invalid architectures

---

## Next Steps (Sprint 1)

1. **This week**:
   - Create initial `system.yaml` with core actors
   - Set up diagram generator
   - Write first contract tests

2. **This month**:
   - Document all message types
   - Implement core runtime (mycelium-core)
   - Add CI validation

3. **This quarter**:
   - Build first adapter (polygon-adapter)
   - Build first strategy (flash-arbitrage)
   - Deploy in monolith mode

**Remember**: The spec is the source of truth. Code follows the spec. CI enforces the match.
