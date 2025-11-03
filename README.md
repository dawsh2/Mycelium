# Mycelium <img src="docs/assets/cultures.svg" alt="Mycelium Logo" width="48" style="vertical-align: middle; margin-bottom: -8px;">

Mycelium is a **type-safe pub/sub messaging system** with topology-based transport selection. Define messages once in YAML, write pub/sub code once, deploy anywhere - transport is inferred from configuration. Same binary runs as single-process (Arc), multi-process (Unix sockets), or distributed (TCP). Built for low-latency systems like high-frequency trading, multiplayer game servers, and real-time analytics.

**Key features:** Schema-driven code generation, bijective zerocopy serialization, ~200ns local latency, can be wrapped in lightweight actor runtime for supervision patterns.

---

## Transport Performance

- **Same node (process)**: `Arc<T>` zero-copy sharing (~200ns latency) - no serialization
- **Different nodes, same host**: Unix domain sockets (~50μs latency) - TLV wire protocol
- **Different nodes, different hosts**: TCP with zero-copy deserialization (~500μs + network) - TLV wire protocol

---

## Quick Start

### 1. Define Messages in contracts.yaml

```yaml
# crates/mycelium-protocol/contracts.yaml
messages:
  PoolStateUpdate:
    tlv_type: 11
    domain: MarketData
    description: "DEX pool state update"
    fields:
      pool_address: { type: "[u8; 20]" }
      reserve0: { type: "U256" }
      reserve1: { type: "U256" }
      block_number: { type: "u64" }
```

Run `cargo build` - messages are **generated automatically** at build time.

### 2. Publish and Subscribe

```rust
use mycelium_protocol::PoolStateUpdate;
use mycelium_transport::MessageBus;

// Create bus (uses Local transport by default)
let bus = MessageBus::new();

// Publisher
let pub_ = bus.publisher::<PoolStateUpdate>();
pub_.publish(PoolStateUpdate { /* ... */ }).await?;

// Subscriber
let mut sub = bus.subscriber::<PoolStateUpdate>();
while let Some(update) = sub.recv().await {
    println!("Pool update: {:?}", update);
}
```

### 3. Configure Deployment Topology

**Single node** (one process):
```toml
# config/dev.toml
[[nodes]]
name = "main"
services = ["polygon-adapter", "flash-arbitrage", "order-executor"]
```

**Multiple nodes, same host** (multiple processes, one machine):
```toml
# config/staging.toml
socket_dir = "/tmp/mycelium"

[[nodes]]
name = "adapters"
services = ["polygon-adapter"]

[[nodes]]
name = "strategies"
services = ["flash-arbitrage"]
# No host specified → uses Unix sockets via socket_dir
```

**Multiple nodes, different hosts** (distributed):
```toml
# config/prod.toml
[[nodes]]
name = "adapters"
services = ["polygon-adapter"]
host = "10.0.1.10"
port = 9000

[[nodes]]
name = "strategies"
services = ["flash-arbitrage"]
host = "10.0.2.20"
port = 9001
# Different hosts → uses TCP
```

**Same code, different configs.** The `MessageBus` reads your topology and infers transport: same node → Arc, same host → Unix, different hosts → TCP.

---

## Architecture

### Core Components

- **`mycelium-protocol`** - Message trait, TYPE_ID, TOPIC, Envelope abstraction
- **`mycelium-config`** - Topology configuration, deployment modes
- **`mycelium-transport`** - Local (Arc), Unix socket, and TCP transports with unified MessageBus API

### Wire Protocol

**Local transport** (same node): No serialization - messages shared via `Arc<T>` clones.

**Remote transports** (Unix/TCP): Type-Length-Value (TLV) protocol with **bijective serialization**:

```
┌──────────┬──────────┬─────────────────┐
│ Type ID  │ Length   │ Payload         │
│ (2 bytes)│ (4 bytes)│ (N bytes)       │
└──────────┴──────────┴─────────────────┘
```

Messages use [zerocopy](https://github.com/google/zerocopy) for **true zero-copy deserialization** - direct memory casting with no allocation or copying overhead.

**Bijective requirement**: Messages must perfectly round-trip (serialize → deserialize = identical). This is enforced by `#[repr(C)]` and zerocopy's `AsBytes`/`FromBytes` traits, enabling zero-copy across process boundaries.

---

## Documentation

- **[docs/TRANSPORT.md](docs/TRANSPORT.md)** - Complete transport architecture, wire protocol, deployment topologies
- **[docs/ACTORS.md](docs/ACTORS.md)** - Actor system evolution path (Phase 1 foundation → Phase 2 full actors)

---

## License

MIT OR Apache-2.0
