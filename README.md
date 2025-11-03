# Mycelium

**Pub/Sub Transport Layer with Adaptive Routing**

Mycelium is a type-safe pub/sub messaging system implemented in Rust that provides **adaptive transport** based on deployment topology. Write your pub/sub code once, and the runtime automatically selects the optimal transport:

- **Same bundle (process)**: `Arc<T>` zero-copy sharing (~200ns latency)
- **Different bundles, same machine**: Unix domain sockets (~50μs latency)  
- **Different machines**: TCP with zero-copy deserialization (~500μs + network)

**Philosophy**: Same code, different deployment configurations. No code changes needed to go from monolith → multi-process → distributed.

---

## Quick Start

### Define a Message

```rust
use mycelium_protocol::impl_message;
use zerocopy::{AsBytes, FromBytes, FromZeroes};

#[derive(Debug, Clone, Copy, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
pub struct SwapEvent {
    pub pool_id: u64,
    pub amount_in: u64,
    pub amount_out: u64,
}

// TYPE_ID: 100, TOPIC: "market-data"
impl_message!(SwapEvent, 100, "market-data");
```

### Publish and Subscribe

```rust
use mycelium_transport::MessageBus;

let bus = MessageBus::new();

// Publisher
let pub_ = bus.publisher::<SwapEvent>();
pub_.publish(SwapEvent {
    pool_id: 123,
    amount_in: 1000,
    amount_out: 2000,
}).await?;

// Subscriber
let mut sub = bus.subscriber::<SwapEvent>();
while let Some(event) = sub.recv().await {
    println!("Swap: {} → {}", event.amount_in, event.amount_out);
}
```

### Configure Deployment Topology

**Monolith** (single process):
```toml
# config/dev.toml
[deployment]
mode = "monolith"

[[bundles]]
name = "main"
services = ["polygon-adapter", "flash-arbitrage", "order-executor"]
```

**Bundled** (multiple processes, one machine):
```toml
# config/staging.toml
[deployment]
mode = "bundled"

[inter_bundle]
transport = "unix"
socket_dir = "/tmp/mycelium"

[[bundles]]
name = "adapters"
services = ["polygon-adapter"]

[[bundles]]
name = "strategies"
services = ["flash-arbitrage"]
```

**Distributed** (multiple machines):
```toml
# config/prod.toml
[deployment]
mode = "distributed"

[[bundles]]
name = "adapters"
services = ["polygon-adapter"]
host = "10.0.1.10"
port = 9000

[[bundles]]
name = "strategies"
services = ["flash-arbitrage"]
host = "10.0.2.20"
port = 9001
```

**Same code, different configs.** The `MessageBus` automatically selects the right transport based on topology.

---

## Architecture

### Core Components

- **`mycelium-protocol`** - Message trait, TYPE_ID, TOPIC, Envelope abstraction
- **`mycelium-config`** - Topology configuration, deployment modes
- **`mycelium-transport`** - Local (Arc), Unix socket, and TCP transports with unified MessageBus API

### Wire Protocol

Remote transports (Unix/TCP) use a simple **Type-Length-Value (TLV)** protocol:

```
┌──────────┬──────────┬─────────────────┐
│ Type ID  │ Length   │ Payload         │
│ (2 bytes)│ (4 bytes)│ (N bytes)       │
└──────────┴──────────┴─────────────────┘
```

Payloads use [zerocopy](https://github.com/google/zerocopy) for **true zero-copy serialization** - direct memory casting with no allocation or copying overhead.

---

## Performance

### Latency (p50)

| Transport | Latency | Use Case |
|-----------|---------|----------|
| Local (Arc) | ~200ns | Same process |
| Unix Socket | ~50μs | Same machine, different processes |
| TCP (localhost) | ~100μs | Testing |
| TCP (LAN) | ~500μs | Same datacenter |

### Throughput

- **Local**: ~5M msg/sec (single pub → single sub)
- **Unix**: ~200K msg/sec  
- **TCP**: ~100K msg/sec (network-bound)

### Memory

- `Publisher<M>`: 8 bytes
- `Subscriber<M>`: 16 bytes
- Message overhead: 0 bytes (Arc) or 6 bytes (TLV header)

---

## Testing

```bash
# Run all tests
cargo test

# Run transport tests only
cargo test -p mycelium-transport

# Check coverage
cargo tarpaulin --out Html
```

**Current status**: 52 tests, 90%+ coverage

---

## Comparison with Related Tools

### vs. ZeroMQ

ZeroMQ is a message queue and transport library. It provides sockets and patterns for message passing, but **always requires serialization**. 

Mycelium differs in that same-process pub/sub uses `Arc<T>` sharing with **zero serialization overhead**.

### vs. Actix

Actix is a Rust actor framework focused on concurrency within a single process. 

Mycelium provides **pub/sub** (not point-to-point actors) and adds **adaptive transport** so the same code can run across processes or machines without changes.

### vs. Erlang/OTP

Erlang/OTP pioneered the actor model for distributed systems with supervision trees. 

Mycelium is **simpler** (pub/sub only, no supervision) and designed for **low-latency HFT applications** with more control over serialization and transports. We rely on systemd/Kubernetes for process management instead of custom supervision.

---

## Project Status

**v0.1.0** - Foundation Complete ✅

- [x] Core message protocol
- [x] Local transport (Arc)
- [x] Unix socket transport
- [x] TCP transport
- [x] TLV wire protocol
- [x] MessageBus with automatic transport selection
- [x] Topology configuration
- [x] Comprehensive test suite

**v0.2.0** - First Service (In Progress 🚧)

- [ ] Polygon market data adapter
- [ ] Health checks and metrics
- [ ] Observability (tracing, Prometheus)

**v0.3.0** - Production Ready (Planned 📋)

- [ ] Flash arbitrage strategy
- [ ] Order executor
- [ ] Load testing (1M msg/sec sustained)
- [ ] Kubernetes deployment

---

## Documentation

- **[TRANSPORT.md](TRANSPORT.md)** - Complete architecture guide and API reference
- **[REWRITE.md](REWRITE.md)** - Development process, timeline, and quality standards
- **[REFACTORING_SUMMARY.md](REFACTORING_SUMMARY.md)** - Code quality improvements

### Research

See `docs/research/` for:
- Actor framework evaluation (Actix, Kameo, Ractor, Bastion)
- Decision rationale for building custom transport layer
- Alternative approaches considered

---

## Design Principles

1. **Simplicity** - Pub/sub only. No actors, no supervision, no request/reply patterns.
2. **Linear data flow** - Events stream one way through the pipeline.
3. **Crash and restart** - Use systemd/Kubernetes for process management.
4. **Configuration over code** - Deployment topology is a config file, not code.
5. **Type safety** - Compile-time guarantees for message types.
6. **Zero-copy** - Minimize allocations and serialization in hot paths.

---

## Contributing

We use **git worktrees** for parallel development:

```bash
# Create feature worktree
git worktree add ~/mycelium/feature-name -b feature/feature-name

# Work in isolation
cd ~/mycelium/feature-name
cargo test
git commit -m "Add feature"

# Merge when ready
cd ~/mycelium
git merge feature/feature-name
```

### Quality Standards

All PRs must include:
- [ ] Tests (90%+ unit coverage, 100% contract coverage)
- [ ] Documentation for public APIs
- [ ] `cargo fmt` and `cargo clippy` passing
- [ ] Updated `system.yaml` if architecture changes

---

## License

MIT OR Apache-2.0 (choose whichever you prefer)

---

## What Mycelium Is Not

- ❌ **Not an actor framework** - We have pub/sub, not point-to-point mailboxes
- ❌ **Not a supervision tree** - Use systemd/k8s for process restarts
- ❌ **Not a distributed consensus system** - Use external coordination if needed
- ❌ **Not a web framework** - Use axum/actix-web if you need HTTP

**Mycelium is**: A simple, fast, type-safe pub/sub transport layer. Nothing more, nothing less. 🦀
