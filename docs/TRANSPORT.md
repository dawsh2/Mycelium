# Mycelium Transport Layer

**Pub/Sub Streaming with Adaptive Transport**

Mycelium enables pub/sub messaging with **topology-based transport selection**:

- **Arc<T>** (same process) → ~200ns latency
- **Unix sockets** (same machine) → ~50μs latency  
- **TCP** (different machines) → ~500μs + network

**Key principle**: Write pub/sub code once. The runtime selects optimal transport based on configuration.

---

## Quick Start

### Define Messages

```rust
use mycelium_protocol::impl_message;
use zerocopy::{AsBytes, FromBytes, FromZeroes};

#[derive(Debug, Clone, Copy, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
pub struct SwapEvent {
    pub pool_id: u64,
    pub amount_in: u128,
    pub amount_out: u128,
}

impl_message!(SwapEvent, 100, "market-data");
```

### Publish & Subscribe

```rust
use mycelium_transport::MessageBus;

// Create bus
let bus = MessageBus::new();

// Publisher
let pub_ = bus.publisher::<SwapEvent>();
pub_.publish(SwapEvent { ... }).await?;

// Subscriber
let mut sub = bus.subscriber::<SwapEvent>();
let event = sub.recv().await.unwrap();
```

---

## Architecture

### Message Protocol

All messages implement `Message` trait:
- `TYPE_ID: u16` - Unique identifier for routing
- `TOPIC: &'static str` - Pub/sub topic name
- Must be `Send + Sync + Clone + AsBytes + FromBytes`

### Transport Types

| Transport | Use Case | Latency | Serialization |
|-----------|----------|---------|---------------|
| **Local** | Same process | ~200ns | None (Arc) |
| **Unix** | Same machine | ~50μs | Zero-copy |
| **TCP** | Different machines | ~500μs+ | Zero-copy |

### Wire Protocol (Unix/TCP)

TLV (Type-Length-Value) format:
```
[type_id: u16][length: u32][payload: zerocopy bytes]
```

---

## Deployment Topologies

### Single Node (Development)

All services in one process:

```toml
[[nodes]]
name = "main"
services = ["adapter", "strategy", "executor"]
```

**Transport**: Local (Arc) only

### Multiple Nodes, Same Host (Staging)

Services grouped into nodes on one machine:

```toml
socket_dir = "/tmp/mycelium"

[[nodes]]
name = "adapters"
services = ["polygon-adapter"]

[[nodes]]
name = "strategies"
services = ["flash-arbitrage"]
```

**Transport**: Arc within nodes, Unix between nodes

### Distributed (Production)

Nodes on different machines:

```toml
[[nodes]]
name = "adapters"
host = "10.0.1.10"
port = 9000

[[nodes]]
name = "strategies"
host = "10.0.2.20"
port = 9001
```

**Transport**: Arc within nodes, TCP between nodes

---

## Configuration API

```rust
use mycelium_config::Topology;

let topology = Topology::load("config/multi-node.toml")?;
let bus = MessageBus::from_topology(topology, "strategies");

// Transport automatically inferred from topology
let pub_ = bus.publisher_to::<SwapEvent>("polygon-adapter").await?;
```

---

## Status

### Completed (v0.1.0)

- ✅ Core message protocol (Message trait, TYPE_ID)
- ✅ Local transport (Arc-based pub/sub)
- ✅ Unix socket transport (IPC)
- ✅ TCP transport (network)
- ✅ Zero-copy serialization (zerocopy)
- ✅ MessageBus with topology-based transport selection
- ✅ Topology configuration (TOML)
- ✅ Test suite (40+ tests passing)

### Next

- [ ] First service: polygon-adapter
- [ ] Observability (tracing, metrics)
- [ ] Production deployment

---

## Design Principles

**Simplicity**: No actor supervision, no request/reply, no consensus. Just pub/sub.

**Linear data flow**: Blockchain → Adapter → Strategy → Executor → Blockchain

**Crash and restart**: Let systemd/k8s handle restarts, not custom supervision.

**Configuration over code**: Same binary, different deployment configs.

---

## Examples

See `examples/simple_pubsub.rs` for a working example.

Run with:
```bash
cargo run --example simple_pubsub
```

---

## Contributing

```
crates/
├── mycelium-protocol/     # Message trait, impl_message! macro
├── mycelium-config/       # Topology, Node
└── mycelium-transport/    # Local, Unix, TCP, MessageBus
```

Run tests:
```bash
cargo test --workspace
```

---

**Mycelium**: Simple, fast, type-safe pub/sub with adaptive transport. 🦀
