# Mycelium Trading System - Project Structure

**Philosophy**: Monorepo with clear separation between core runtime and trading domain logic.

```
mycelium/
├── README.md                           # Project overview
├── Cargo.toml                          # Workspace root
├── Cargo.lock                          # Locked dependencies
├── rust-toolchain.toml                 # Rust version specification
├── .gitignore
├── .pre-commit-config.yaml             # Pre-commit hooks
│
├── docs/                               # 📚 Documentation
│   ├── ARCHITECTURE.md                 # High-level architecture
│   ├── REWRITE.md                      # v2 rewrite plan
│   ├── CONTRIBUTING.md                 # Contribution guidelines
│   ├── CHANGELOG.md                    # Version history
│   │
│   ├── GUARDRAILS/                     # 🛡️ Architecture enforcement
│   │   ├── README.md                   # Guardrails philosophy
│   │   ├── system.yaml                 # ⭐ Canonical architecture spec
│   │   ├── testing-standards.md        # Testing requirements
│   │   ├── config-vs-spec.md          # Config vs spec distinction
│   │   │
│   │   ├── diagrams/                   # Generated from system.yaml
│   │   │   ├── actor-topology.mmd
│   │   │   ├── message-flow.mmd
│   │   │   └── deployment-modes.mmd
│   │   │
│   │   ├── specs/                      # Data specifications
│   │   │   ├── messages.md             # Message type specs
│   │   │   ├── actor-contracts.md      # Actor interface contracts
│   │   │   └── deployment.md           # Deployment topology spec
│   │   │
│   │   └── generator/                  # Diagram generation tools
│   │       ├── generate_diagrams.py
│   │       └── validate_system.py      # CI validation
│   │
│   ├── architecture/                   # Architecture deep-dives
│   │   ├── actor-model.md
│   │   ├── message-protocol.md
│   │   ├── deployment-topology.md
│   │   └── fault-tolerance.md
│   │
│   ├── guides/                         # User guides
│   │   ├── quickstart.md
│   │   ├── writing-actors.md
│   │   ├── deploying.md
│   │   └── monitoring.md
│   │
│   └── api/                            # API documentation (generated)
│       └── .gitkeep
│
├── crates/                             # 📦 Library crates
│   │
│   ├── runtime/                        # 🧠 Core Mycelium Runtime
│   │   ├── mycelium-core/             # Actor system core
│   │   │   ├── Cargo.toml
│   │   │   ├── src/
│   │   │   │   ├── lib.rs
│   │   │   │   ├── actor.rs            # Actor trait
│   │   │   │   ├── mailbox.rs          # Message queue
│   │   │   │   ├── supervision.rs      # Supervision strategies
│   │   │   │   ├── lifecycle.rs        # Actor lifecycle
│   │   │   │   └── context.rs          # Actor context
│   │   │   └── tests/
│   │   │       └── actor_tests.rs
│   │   │
│   │   ├── mycelium-protocol/         # Message protocol
│   │   │   ├── Cargo.toml
│   │   │   ├── src/
│   │   │   │   ├── lib.rs
│   │   │   │   ├── message.rs          # Message trait
│   │   │   │   ├── envelope.rs         # Message envelope
│   │   │   │   ├── codec.rs            # Serialization
│   │   │   │   └── types.rs            # Common message types
│   │   │   └── tests/
│   │   │       └── roundtrip_tests.rs  # Contract tests
│   │   │
│   │   ├── mycelium-transport/        # Transport layer
│   │   │   ├── Cargo.toml
│   │   │   ├── src/
│   │   │   │   ├── lib.rs
│   │   │   │   ├── local.rs            # Arc<T> transport
│   │   │   │   ├── unix.rs             # Unix socket transport
│   │   │   │   ├── tcp.rs              # TCP transport
│   │   │   │   └── adaptive.rs         # Adaptive selection
│   │   │   └── tests/
│   │   │       └── transport_tests.rs
│   │   │
│   │   ├── mycelium-config/           # Configuration system
│   │   │   ├── Cargo.toml
│   │   │   ├── src/
│   │   │   │   ├── lib.rs
│   │   │   │   ├── profile.rs          # Deployment profiles
│   │   │   │   ├── bundle.rs           # Actor bundles
│   │   │   │   ├── topology.rs         # Network topology
│   │   │   │   └── loader.rs           # Config loading
│   │   │   └── tests/
│   │   │       └── config_tests.rs
│   │   │
│   │   └── mycelium-runtime/          # Runtime orchestration
│   │       ├── Cargo.toml
│   │       ├── src/
│   │       │   ├── lib.rs
│   │       │   ├── runtime.rs          # Runtime manager
│   │       │   ├── scheduler.rs        # Actor scheduling
│   │       │   └── registry.rs         # Actor registry
│   │       └── tests/
│   │           └── runtime_tests.rs
│   │
│   ├── protocol/                       # 📡 Trading Protocol
│   │   ├── mycelium-messages/         # TLV message definitions
│   │   │   ├── Cargo.toml
│   │   │   ├── src/
│   │   │   │   ├── lib.rs
│   │   │   │   ├── market_data.rs      # Market data messages
│   │   │   │   ├── signals.rs          # Trading signals
│   │   │   │   ├── execution.rs        # Execution messages
│   │   │   │   └── admin.rs            # Admin/control messages
│   │   │   └── tests/
│   │   │       └── message_contracts.rs # 100% coverage required
│   │   │
│   │   └── mycelium-codec/            # Zero-copy codec
│   │       ├── Cargo.toml
│   │       ├── src/
│   │       │   ├── lib.rs
│   │       │   ├── tlv.rs              # TLV encoding/decoding
│   │       │   ├── zerocopy.rs         # Zero-copy utilities
│   │       │   └── schema.rs           # Schema versioning
│   │       └── tests/
│   │           └── codec_tests.rs
│   │
│   ├── domain/                         # 💼 Trading Domain Logic
│   │   ├── mycelium-defi/             # DeFi primitives
│   │   │   ├── Cargo.toml
│   │   │   ├── src/
│   │   │   │   ├── lib.rs
│   │   │   │   ├── amm/
│   │   │   │   │   ├── mod.rs
│   │   │   │   │   ├── v2.rs           # Uniswap V2 math
│   │   │   │   │   └── v3.rs           # Uniswap V3 math
│   │   │   │   ├── pool.rs             # Pool types
│   │   │   │   └── token.rs            # Token types
│   │   │   └── tests/
│   │   │       └── amm_tests.rs        # Property tests
│   │   │
│   │   ├── mycelium-strategy/         # Strategy framework
│   │   │   ├── Cargo.toml
│   │   │   ├── src/
│   │   │   │   ├── lib.rs
│   │   │   │   ├── strategy.rs         # Strategy trait
│   │   │   │   ├── portfolio.rs        # Portfolio management
│   │   │   │   └── risk.rs             # Risk management
│   │   │   └── tests/
│   │   │       └── strategy_tests.rs
│   │   │
│   │   └── mycelium-execution/        # Execution logic
│   │       ├── Cargo.toml
│   │       ├── src/
│   │       │   ├── lib.rs
│   │       │   ├── order.rs            # Order types
│   │       │   ├── signer.rs           # Transaction signing
│   │       │   └── gas.rs              # Gas estimation
│   │       └── tests/
│   │           └── execution_tests.rs
│   │
│   ├── infrastructure/                 # 🔧 Infrastructure
│   │   ├── mycelium-storage/          # Storage abstractions
│   │   │   ├── Cargo.toml
│   │   │   ├── src/
│   │   │   │   ├── lib.rs
│   │   │   │   ├── cache.rs            # In-memory cache
│   │   │   │   ├── redis.rs            # Redis backend
│   │   │   │   └── postgres.rs         # Postgres backend
│   │   │   └── tests/
│   │   │       └── storage_tests.rs
│   │   │
│   │   └── mycelium-observability/    # Metrics & tracing
│   │       ├── Cargo.toml
│   │       ├── src/
│   │       │   ├── lib.rs
│   │       │   ├── metrics.rs          # Prometheus metrics
│   │       │   ├── tracing.rs          # Distributed tracing
│   │       │   └── health.rs           # Health checks
│   │       └── tests/
│   │           └── observability_tests.rs
│   │
│   └── testing/                        # 🧪 Testing Utilities
│       ├── mycelium-testkit/          # Test utilities
│       │   ├── Cargo.toml
│       │   ├── src/
│       │   │   ├── lib.rs
│       │   │   ├── mock_actor.rs       # Mock actors
│       │   │   ├── fixtures.rs         # Test data
│       │   │   └── assertions.rs       # Custom assertions
│       │   └── tests/
│       │       └── testkit_tests.rs
│       │
│       └── mycelium-backtest/         # Backtesting engine
│           ├── Cargo.toml
│           ├── src/
│           │   ├── lib.rs
│           │   ├── engine.rs           # Backtest engine
│           │   ├── parallel.rs         # Parallel execution
│           │   └── replay.rs           # Event replay
│           └── tests/
│               └── backtest_tests.rs
│
├── services/                           # 🎯 Actor Services (Binaries)
│   │
│   ├── adapters/                       # Data ingestion actors
│   │   ├── polygon-adapter/
│   │   │   ├── Cargo.toml
│   │   │   ├── src/
│   │   │   │   ├── main.rs             # Binary entry point
│   │   │   │   ├── actor.rs            # Adapter actor
│   │   │   │   ├── websocket.rs        # WS connection
│   │   │   │   └── discovery.rs        # Lazy discovery
│   │   │   └── tests/
│   │   │       └── adapter_tests.rs
│   │   │
│   │   └── ethereum-adapter/
│   │       └── ... (similar structure)
│   │
│   ├── strategies/                     # Trading strategy actors
│   │   ├── flash-arbitrage/
│   │   │   ├── Cargo.toml
│   │   │   ├── src/
│   │   │   │   ├── main.rs
│   │   │   │   ├── detector.rs         # Opportunity detection
│   │   │   │   ├── calculator.rs       # Profit calculation
│   │   │   │   └── router.rs           # Route optimization
│   │   │   └── tests/
│   │   │       └── strategy_tests.rs
│   │   │
│   │   └── ml-strategy/
│   │       └── ... (future)
│   │
│   ├── execution/                      # Execution actors
│   │   └── order-manager/
│   │       ├── Cargo.toml
│   │       ├── src/
│   │       │   ├── main.rs
│   │       │   ├── manager.rs          # Order lifecycle
│   │       │   └── risk_engine.rs      # Pre-trade risk
│   │       └── tests/
│   │           └── execution_tests.rs
│   │
│   └── infrastructure/                 # Infrastructure actors
│       ├── relay/                      # Message relay
│       │   ├── Cargo.toml
│       │   ├── src/
│       │   │   ├── main.rs
│       │   │   └── relay.rs
│       │   └── tests/
│       │       └── relay_tests.rs
│       │
│       └── state-subscriber/          # State persistence
│           ├── Cargo.toml
│           ├── src/
│           │   ├── main.rs
│           │   └── subscriber.rs
│           └── tests/
│               └── subscriber_tests.rs
│
├── config/                             # ⚙️ Configuration
│   ├── profiles/                       # Deployment profiles
│   │   ├── development.toml            # Local dev (monolith)
│   │   ├── staging.toml                # Staging (multi-process)
│   │   └── production.toml             # Production (distributed)
│   │
│   ├── chains/                         # Chain configurations
│   │   ├── polygon.toml
│   │   └── ethereum.toml
│   │
│   └── actors/                         # Actor-specific config
│       ├── polygon-adapter.toml
│       └── flash-arbitrage.toml
│
├── tests/                              # 🧪 Integration & E2E Tests
│   ├── contract/                       # Contract tests
│   │   ├── message_roundtrip.rs
│   │   └── actor_contracts.rs
│   │
│   ├── integration/                    # Integration tests
│   │   ├── adapter_strategy.rs
│   │   └── full_pipeline.rs
│   │
│   ├── e2e/                           # End-to-end tests
│   │   ├── docker-compose.yml
│   │   └── full_system_test.rs
│   │
│   ├── replays/                       # Replay tests (golden files)
│   │   ├── 2025-11-02-baseline/
│   │   │   ├── README.md
│   │   │   ├── events.json
│   │   │   └── expected_signals.json
│   │   └── ... (dated scenarios)
│   │
│   └── fixtures/                      # Test data
│       ├── pools/
│       ├── tokens/
│       └── events/
│
├── scripts/                           # 🔧 Utility Scripts
│   ├── start                          # Start services
│   ├── stop                           # Stop services
│   ├── validate_system.sh             # Validate system.yaml
│   ├── generate_diagrams.sh           # Generate architecture diagrams
│   └── benchmark.sh                   # Run benchmarks
│
├── tools/                             # 🛠️ Development Tools
│   ├── backtest/                      # Backtesting CLI
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── main.rs
│   │
│   └── diagnostics/                   # Diagnostic tools
│       ├── Cargo.toml
│       └── src/
│           └── main.rs
│
├── benches/                           # 📊 Benchmarks
│   ├── message_passing.rs
│   ├── actor_spawn.rs
│   └── transport_latency.rs
│
├── examples/                          # 📖 Examples
│   ├── simple_actor.rs
│   ├── message_passing.rs
│   └── deployment_modes.rs
│
├── deployments/                       # 🚀 Deployment Configs
│   ├── docker/
│   │   ├── Dockerfile.adapter
│   │   ├── Dockerfile.strategy
│   │   └── docker-compose.yml
│   │
│   └── k8s/                           # Kubernetes manifests
│       ├── namespace.yaml
│       ├── adapter-deployment.yaml
│       └── strategy-deployment.yaml
│
└── .github/                           # 🤖 GitHub Actions
    ├── workflows/
    │   ├── ci.yml                     # PR checks
    │   ├── release.yml                # Release automation
    │   └── nightly.yml                # Nightly tests
    │
    └── PULL_REQUEST_TEMPLATE.md       # PR template
```

---

## Key Design Decisions

### 1. Workspace Structure
```toml
# Cargo.toml (root)
[workspace]
members = [
    # Runtime
    "crates/runtime/mycelium-core",
    "crates/runtime/mycelium-protocol",
    "crates/runtime/mycelium-transport",
    "crates/runtime/mycelium-config",
    "crates/runtime/mycelium-runtime",

    # Protocol
    "crates/protocol/mycelium-messages",
    "crates/protocol/mycelium-codec",

    # Domain
    "crates/domain/mycelium-defi",
    "crates/domain/mycelium-strategy",
    "crates/domain/mycelium-execution",

    # Infrastructure
    "crates/infrastructure/mycelium-storage",
    "crates/infrastructure/mycelium-observability",

    # Testing
    "crates/testing/mycelium-testkit",
    "crates/testing/mycelium-backtest",

    # Services
    "services/adapters/polygon-adapter",
    "services/strategies/flash-arbitrage",
    "services/execution/order-manager",
    "services/infrastructure/relay",
    "services/infrastructure/state-subscriber",

    # Tools
    "tools/backtest",
    "tools/diagnostics",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
rust-version = "1.75"

[workspace.dependencies]
# Shared dependencies
tokio = { version = "1.35", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
anyhow = "1.0"
tracing = "0.1"
```

### 2. Clear Separation of Concerns

**Runtime** (`crates/runtime/`)
- Pure actor model implementation
- Transport-agnostic
- No trading domain logic
- Could be used for any actor-based system

**Protocol** (`crates/protocol/`)
- Trading-specific message types
- TLV codec implementation
- Schema versioning

**Domain** (`crates/domain/`)
- Trading logic (AMM math, strategies)
- Independent of actor runtime
- Can be tested in isolation

**Services** (`services/`)
- Concrete actor implementations
- Binary crates (have `main.rs`)
- Deployable units

### 3. GUARDRAILS from Day One

**docs/GUARDRAILS/** enforces:
- `system.yaml` = canonical architecture
- Contract tests for all messages (100% coverage)
- CI validation before merge
- Spec-to-code sync

### 4. Testing Strategy

**Unit tests**: Co-located with code (`src/*_test.rs`)
**Contract tests**: `tests/contract/` - message roundtrips
**Integration tests**: `tests/integration/` - actor pairs
**Replay tests**: `tests/replays/` - golden scenarios
**E2E tests**: `tests/e2e/` - full system

### 5. Configuration Hierarchy

```
config/
├── profiles/           # How to deploy (monolith/distributed)
├── chains/             # Blockchain configs
└── actors/             # Actor-specific settings
```

Loaded in order:
1. Default embedded in code
2. Chain config
3. Profile config
4. Actor config
5. Environment variables (override)

---

## First Steps (Sprint 1)

1. **Initialize workspace**
   ```bash
   cargo new --lib crates/runtime/mycelium-core
   cargo new --lib crates/protocol/mycelium-messages
   cargo new --lib crates/testing/mycelium-testkit
   ```

2. **Create GUARDRAILS**
   ```bash
   mkdir -p docs/GUARDRAILS
   touch docs/GUARDRAILS/system.yaml
   ```

3. **Set up CI**
   ```bash
   mkdir -p .github/workflows
   # Create ci.yml
   ```

4. **Write first contract test**
   ```rust
   // tests/contract/message_roundtrip.rs
   #[test]
   fn message_roundtrips_correctly() {
       // Defines expected behavior BEFORE implementation
   }
   ```

---

## Dependencies Between Crates

```
mycelium-runtime
    ↓
mycelium-transport ← mycelium-protocol ← mycelium-core
    ↓                       ↓
mycelium-messages    mycelium-codec
    ↓
services/* (actors)
```

**Dependency rules**:
- Runtime never depends on domain
- Protocol never depends on domain
- Domain can depend on protocol
- Services depend on everything

---

## Naming Conventions

**Crates**: `mycelium-<name>` (all lowercase, hyphenated)
**Binaries**: `<domain>-<service>` (e.g., `polygon-adapter`)
**Files**: `snake_case.rs`
**Modules**: `snake_case`
**Types**: `PascalCase`
**Functions**: `snake_case`

---

This structure supports:
- ✅ Incremental development (can work on runtime independently of domain)
- ✅ Parallel development (teams work on different crates)
- ✅ Clear testing boundaries
- ✅ GUARDRAILS enforcement from commit #1
- ✅ Deployment flexibility (monolith → distributed)
- ✅ Future growth (new chains, strategies, protocols)
