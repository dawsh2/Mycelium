# Mycelium

**Messaging System for Rust services, with configurable deployment topology and transport mechanisms.**

Mycelium provides direct service-to-service communication with automatic transport selection. Services publish and subscribe to typed messages. The transport layer (Arc/Unix/TCP) is determined by configuration, not code.

## Core Concept

Same service code works in different deployment topologies:

```toml
# Development: everything in one process
[deployment]
mode = "monolith"

# Production: services in bundles, isolated by process
[deployment]
mode = "bundled"

[[bundles]]
name = "adapters"
services = ["polygon-adapter", "ethereum-adapter"]

[[bundles]]
name = "strategies"
services = ["flash-arbitrage"]

# Distributed: services on different machines
[deployment]
mode = "distributed"

[[bundles]]
name = "execution"
host = "192.168.1.100"
port = 9002
services = ["order-manager"]
```

The MessageBus automatically selects transport:
- **Same bundle**: `Arc<T>` (~50ns)
- **Different bundle, same machine**: Unix sockets (~2μs)
- **Different machine**: TCP (~10-100μs)

## Example

### Define Messages

```rust
use mycelium_protocol::impl_message;
use rkyv::{Archive, Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Archive, Serialize, Deserialize)]
#[archive(check_bytes)]
struct SwapEvent {
    pool_id: u64,
    amount_in: u128,
    amount_out: u128,
    timestamp: u64,
}

impl_message!(SwapEvent, 1, "market-data");
```

### Write Services

```rust
use mycelium_transport::MessageBus;

#[tokio::main]
async fn main() -> Result<()> {
    let topology = Topology::load("config/topology.toml")?;
    let bus = MessageBus::from_topology(topology, "flash-arbitrage");

    // Subscribe to events from polygon-adapter
    let mut swaps = bus.subscriber_from::<SwapEvent>("polygon-adapter").await?;

    // Publish signals to order-manager
    let signals = bus.publisher_to::<ArbitrageSignal>("order-manager").await?;

    // Business logic
    while let Some(swap) = swaps.recv().await {
        if let Some(signal) = detect_arbitrage(&swap) {
            signals.publish(signal).await?;
        }
    }

    Ok(())
}
```

The `subscriber_from()` and `publisher_to()` methods look up services in the topology and select the appropriate transport. Change the topology file, not the code.

## Bundling

Services in the same bundle share memory via `Arc<T>`. Services in different bundles communicate via Unix sockets or TCP.

```toml
# Bundle hot path together for Arc transport
[[bundles]]
name = "market-data"
services = ["polygon-adapter", "price-aggregator", "market-maker"]

# Isolate execution in separate bundle
[[bundles]]
name = "execution"
services = ["order-manager"]

[inter_bundle]
transport = "unix"
socket_dir = "/tmp/mycelium"
```

Bundling decisions are configuration, not code changes.

## Performance

| Transport | Latency | When Used |
|-----------|---------|-----------|
| Arc | ~50ns | Services in same bundle |
| Unix socket | ~2μs | Services in different bundles, same machine |
| TCP | ~10-100μs | Services on different machines |

Messages are serialized with rkyv (zero-copy deserialization). Arc transport has no serialization overhead.

## Architecture

```
mycelium/
├── crates/
│   ├── mycelium-protocol/     # Message trait, rkyv serialization
│   ├── mycelium-config/        # Topology parsing, bundle resolution
│   └── mycelium-transport/     # MessageBus, Local/Unix/TCP transports
├── tests/
│   └── integration_test.rs     # End-to-end deployment tests
└── examples/
    └── simple_pubsub.rs        # Basic usage
```

### Key Components

**MessageBus** - Main API for pub/sub messaging
- `publisher<M>()` - Local publisher (any deployment mode)
- `subscriber<M>()` - Local subscriber (any deployment mode)
- `publisher_to<M>(service)` - Smart publisher with auto transport selection
- `subscriber_from<M>(service)` - Smart subscriber with auto transport selection

**Topology** - Configuration model
- Deployment modes: monolith, bundled, distributed
- Bundle definitions with service lists
- Inter-bundle transport configuration
- Host/port for distributed services

**Transport Implementations**
- `LocalTransport` - Arc-based, zero-copy within process
- `UnixTransport` - Unix domain sockets with rkyv serialization
- `TcpTransport` - TCP sockets with rkyv serialization

**AnyPublisher / AnySubscriber** - Transport-agnostic wrappers
- Enum variants: Local, Unix, Tcp
- Unified publish/recv interface
- Transport type is runtime-determined from topology

## Testing

```bash
cargo test                              # All tests
cargo test -p mycelium-transport        # Transport tests only
cargo test --test integration_test      # Integration tests
```

Test coverage: 35 unit tests, 4 integration tests (monolith, bundled, distributed, mixed).

## Use Cases

**Development**: Monolith mode with Arc transport for fast iteration and debugging.

**Production**: Bundled mode with process isolation. Group frequently-communicating services in the same bundle for Arc transport. Isolate critical components in separate bundles.

**Parallel Backtesting**: Spawn multiple processes, each testing different parameters with isolated state.

**Horizontal Scaling**: Move services to different machines by updating topology configuration.

## License

MIT OR Apache-2.0
