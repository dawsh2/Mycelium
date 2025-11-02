# Mycelium Project Structure (Revised)

**Philosophy**: Flat crate structure with clear naming conventions. Let Cargo.toml define relationships, not directory hierarchy.

## Simplified Structure

```
mycelium/
├── README.md
├── Cargo.toml                    # Workspace definition
├── Cargo.lock
├── rust-toolchain.toml
│
├── crates/                       # 📦 All crates (libs + bins) flat
│   │
│   ├── mycelium-core/           # Actor system core
│   ├── mycelium-protocol/       # Message protocol
│   ├── mycelium-transport/      # Transport layer
│   ├── mycelium-config/         # Configuration
│   ├── mycelium-runtime/        # Runtime orchestration
│   │
│   ├── mycelium-messages/       # TLV message definitions
│   ├── mycelium-codec/          # Zero-copy codec
│   │
│   ├── mycelium-defi/           # DeFi primitives
│   ├── mycelium-strategy/       # Strategy framework
│   ├── mycelium-execution/      # Execution logic
│   │
│   ├── mycelium-storage/        # Storage abstractions
│   ├── mycelium-observability/  # Metrics & tracing
│   │
│   ├── mycelium-testkit/        # Test utilities
│   ├── mycelium-backtest/       # Backtesting engine
│   │
│   ├── polygon-adapter/         # 🎯 Polygon blockchain adapter (binary)
│   ├── ethereum-adapter/        # 🎯 Ethereum adapter (binary)
│   ├── flash-arbitrage/         # 🎯 Flash arbitrage strategy (binary)
│   ├── order-manager/           # 🎯 Order execution (binary)
│   ├── relay/                   # 🎯 Message relay (binary)
│   └── state-subscriber/        # 🎯 State persistence (binary)
│
├── docs/                         # 📚 Documentation
│   ├── ARCHITECTURE.md
│   ├── REWRITE.md
│   ├── CONTRIBUTING.md
│   │
│   ├── GUARDRAILS/              # 🛡️ Architecture enforcement
│   │   ├── README.md
│   │   ├── system.yaml          # ⭐ Canonical spec
│   │   ├── testing-standards.md
│   │   ├── diagrams/
│   │   ├── specs/
│   │   └── generator/
│   │
│   ├── guides/
│   └── api/
│
├── config/                       # ⚙️ Configuration
│   ├── profiles/
│   │   ├── development.toml
│   │   ├── staging.toml
│   │   └── production.toml
│   ├── chains/
│   │   ├── polygon.toml
│   │   └── ethereum.toml
│   └── actors/
│       ├── polygon-adapter.toml
│       └── flash-arbitrage.toml
│
├── tests/                        # 🧪 Integration & E2E
│   ├── contract/
│   ├── integration/
│   ├── e2e/
│   ├── replays/
│   └── fixtures/
│
├── scripts/                      # 🔧 Utility scripts
│   ├── start
│   ├── validate_system.sh
│   └── generate_diagrams.sh
│
├── tools/                        # 🛠️ Standalone tools
│   ├── backtest/
│   └── diagnostics/
│
├── benches/                      # 📊 Benchmarks
│   ├── message_passing.rs
│   └── actor_spawn.rs
│
├── examples/                     # 📖 Examples
│   ├── simple_actor.rs
│   └── deployment_modes.rs
│
└── .github/                      # 🤖 CI/CD
    └── workflows/
```

## Key Principles

### 1. Everything in `crates/` (Flat)

**No separation by type:**
- Libraries and binaries live together
- Cargo.toml defines what's what
- Naming convention indicates purpose

**Naming Convention:**
- `mycelium-<name>` = Core runtime/framework (libraries)
- `<domain>-<service>` = Deployable services (binaries)

```toml
# Library crate
# crates/mycelium-core/Cargo.toml
[package]
name = "mycelium-core"

[lib]
name = "mycelium_core"
path = "src/lib.rs"

# Binary crate
# crates/polygon-adapter/Cargo.toml
[package]
name = "polygon-adapter"

[[bin]]
name = "polygon-adapter"
path = "src/main.rs"

# Binary + Library (both!)
# crates/flash-arbitrage/Cargo.toml
[package]
name = "flash-arbitrage"

[lib]
name = "flash_arbitrage"
path = "src/lib.rs"

[[bin]]
name = "flash-arbitrage"
path = "src/main.rs"
```

### 2. Clear Crate Purpose from Name

**Pattern**: `{scope}-{purpose}`

- `mycelium-*` = Core framework (reusable)
- `{chain}-adapter` = Blockchain adapters
- `{strategy-name}` = Trading strategies
- `{service-name}` = Infrastructure services

### 3. Workspace Definition

```toml
# Cargo.toml (root)
[workspace]
members = [
    # Core runtime (libraries)
    "crates/mycelium-core",
    "crates/mycelium-protocol",
    "crates/mycelium-transport",
    "crates/mycelium-config",
    "crates/mycelium-runtime",

    # Protocol (libraries)
    "crates/mycelium-messages",
    "crates/mycelium-codec",

    # Domain logic (libraries)
    "crates/mycelium-defi",
    "crates/mycelium-strategy",
    "crates/mycelium-execution",

    # Infrastructure (libraries)
    "crates/mycelium-storage",
    "crates/mycelium-observability",

    # Testing (libraries)
    "crates/mycelium-testkit",
    "crates/mycelium-backtest",

    # Services (binaries)
    "crates/polygon-adapter",
    "crates/ethereum-adapter",
    "crates/flash-arbitrage",
    "crates/order-manager",
    "crates/relay",
    "crates/state-subscriber",

    # Tools
    "tools/backtest",
    "tools/diagnostics",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
rust-version = "1.75"
authors = ["Your Name <you@example.com>"]
license = "MIT OR Apache-2.0"

[workspace.dependencies]
# Shared dependencies
tokio = { version = "1.35", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
anyhow = "1.0"
tracing = "0.1"

# Internal dependencies
mycelium-core = { path = "crates/mycelium-core" }
mycelium-protocol = { path = "crates/mycelium-protocol" }
mycelium-messages = { path = "crates/mycelium-messages" }
```

### 4. Typical Crate Structure

```
crates/mycelium-core/
├── Cargo.toml
├── src/
│   ├── lib.rs              # Public API
│   ├── actor.rs            # Actor trait
│   ├── mailbox.rs
│   └── supervision.rs
├── tests/
│   └── actor_tests.rs      # Integration tests
├── benches/
│   └── actor_spawn.rs      # Benchmarks
└── examples/
    └── simple_actor.rs     # Usage examples
```

```
crates/polygon-adapter/
├── Cargo.toml
├── src/
│   ├── main.rs            # Binary entry point
│   ├── lib.rs             # Library (reusable logic)
│   ├── actor.rs
│   └── websocket.rs
└── tests/
    └── adapter_tests.rs
```

## Comparison with Torq's `libs/`

**Torq structure:**
```
torq/
├── libs/               # Shared libraries
│   ├── defi/
│   ├── codec/
│   └── ...
├── services/           # Binaries
│   ├── adapters/
│   └── strategies/
└── Cargo.toml
```

**Was this good?**
- ✅ Clear separation of reusable logic
- ⚠️ Extra nesting (libs/, services/)
- ⚠️ Non-standard (most Rust projects use crates/)

**Mycelium approach:**
```
mycelium/
├── crates/             # Everything flat
│   ├── mycelium-defi/      (was libs/defi)
│   ├── mycelium-codec/     (was libs/codec)
│   ├── polygon-adapter/    (was services/adapters/polygon)
│   └── flash-arbitrage/    (was services/strategies/flash)
└── Cargo.toml
```

**Benefits:**
- ✅ Standard Rust pattern
- ✅ Less nesting = easier navigation
- ✅ Clear from naming (`mycelium-*` vs `polygon-*`)
- ✅ Cargo manages dependencies, not directories

## When to Use Subcategories?

**Only if you have 50+ crates**, consider ONE level:

```
crates/
├── runtime/           # If you have 10+ runtime crates
│   ├── core/
│   ├── protocol/
│   └── ...
└── adapters/          # If you have 10+ adapters
    ├── polygon/
    ├── ethereum/
    └── ...
```

**For Mycelium**: Start flat. Add categories only when navigation becomes painful (probably never).

## Finding Things

**How do developers find what they need?**

1. **Naming convention** - `mycelium-` prefix = framework
2. **README.md** - List all crates with descriptions
3. **Cargo.toml** - Workspace members grouped logically
4. **IDE** - File tree search by name
5. **docs/GUARDRAILS/system.yaml** - Canonical architecture map

## Migration Path

If we outgrow flat structure:

```bash
# Easy to reorganize later
git mv crates/mycelium-core crates/runtime/core
git mv crates/mycelium-protocol crates/runtime/protocol
# Update Cargo.toml workspace members
```

Flat structure doesn't lock you in.

---

## Recommendation

**Use flat `crates/` directory with clear naming.**

**Advantages:**
- ✅ Standard Rust pattern (matches tokio, serde, bevy)
- ✅ Less cognitive overhead (no directory categories to remember)
- ✅ Easier refactoring (just rename crate, not move directories)
- ✅ Works with any IDE/editor
- ✅ Cargo manages complexity, not filesystem

**Naming handles organization:**
- `mycelium-core` - obviously core framework
- `polygon-adapter` - obviously blockchain adapter
- `flash-arbitrage` - obviously trading strategy

Simple, clear, standard.
