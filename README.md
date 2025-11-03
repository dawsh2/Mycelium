# Mycelium

**Pub/Sub Transport Layer with Adaptive Routing**

Mycelium is a type-safe pub/sub messaging system implemented in Rust that provides **adaptive transport** based on deployment topology. Write your pub/sub code once, configure your topology, and the transport is selected automatically:

- **Same bundle (process)**: `Arc<T>` zero-copy sharing (~200ns latency)
- **Different bundles, same machine**: Unix domain sockets (~50μs latency)
- **Different machines**: TCP with zero-copy deserialization (~500μs + network)

**Applications**: High-frequency trading, multiplayer game servers, real-time analytics pipelines, IoT/sensor networks.

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

**Same code, different configs.** The `MessageBus` reads your topology configuration and selects the appropriate transport.

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

## Documentation

See **[docs/TRANSPORT.md](docs/TRANSPORT.md)** for complete architecture details and API reference.

---

## License

MIT OR Apache-2.0
