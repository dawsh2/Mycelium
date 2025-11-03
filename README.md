# Mycelium

**Configuration-driven pub/sub message bus with adaptive transport for Rust services.**

Mycelium eliminates the need for message brokers by providing direct service-to-service communication with automatic transport selection based on deployment topology. Write your service code once, then deploy as a monolith, multi-process system, or distributed across machines—all controlled by configuration.

## Why Mycelium?

**Problem**: Traditional message brokers add latency, complexity, and operational overhead.

**Solution**: Direct pub/sub with adaptive transport:
- **Same bundle**: Zero-copy `Arc<T>` sharing (~50ns)
- **Different bundles, same machine**: Unix sockets with rkyv (~2μs)
- **Different machines**: TCP with rkyv (~10-100μs)

Your code doesn't change—only the configuration determines which transport is used.

## Key Features

- ✅ **Adaptive Transport**: Automatic Arc/Unix/TCP selection based on service location
- ✅ **Bundle-Aware**: Group services for shared memory, isolate for fault tolerance
- ✅ **Zero-Copy**: Arc sharing within bundles, zero-copy deserialization with rkyv
- ✅ **Type-Safe**: Compile-time message type checking
- ✅ **Configuration-Driven**: Same code, different deployments
- ✅ **Simple**: Pub/sub semantics, no actor complexity

## Quick Start

### 1. Define Your Messages

```rust
use mycelium_protocol::{impl_message, Message};
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

### 2. Write Your Service

```rust
use mycelium_transport::MessageBus;

#[tokio::main]
async fn main() -> Result<()> {
    // Load topology from config
    let bus = MessageBus::from_topology(topology, "flash-arbitrage");

    // Subscribe to events (transport chosen automatically)
    let mut swaps = bus.subscriber_from::<SwapEvent>("polygon-adapter").await?;

    // Publish signals
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

### 3. Configure Deployment

```toml
# config/development.toml - Everything in one process
[deployment]
mode = "monolith"

[bundles]
name = "main"
services = ["polygon-adapter", "flash-arbitrage", "order-manager"]
```

```toml
# config/production.toml - Bundled with isolation
[deployment]
mode = "bundled"

# Bundle 1: Market data services (shared memory)
[[bundles]]
name = "adapters"
services = ["polygon-adapter", "ethereum-adapter"]

# Bundle 2: Strategy services (shared memory)
[[bundles]]
name = "strategies"
services = ["flash-arbitrage", "market-maker"]

# Bundle 3: Execution (isolated)
[[bundles]]
name = "execution"
services = ["order-manager"]

# Inter-bundle communication via Unix sockets
[inter_bundle]
transport = "unix"
socket_dir = "/tmp/mycelium"
```

```toml
# config/distributed.toml - Across machines
[deployment]
mode = "distributed"

[[bundles]]
name = "adapters"
services = ["polygon-adapter"]
host = "127.0.0.1"
port = 9001

[[bundles]]
name = "strategies"
services = ["flash-arbitrage"]
host = "192.168.1.100"
port = 9002
```

**Same service code runs in all three modes.** Change configuration, not code.

## Architecture

### Deployment Modes

| Mode | Description | Transport | Use Case |
|------|-------------|-----------|----------|
| **Monolith** | All services in one process | Arc (~50ns) | Development, fast iteration |
| **Bundled** | Services grouped in bundles | Arc within bundle, Unix between | Production with fault isolation |
| **Distributed** | Services across machines | TCP | Horizontal scaling, colocation |

### Bundling Strategy

Bundles let you balance performance vs isolation:

```rust
// Services in same bundle
polygon-adapter → flash-arbitrage
    (Arc sharing, ~50ns, zero-copy)

// Services in different bundles
flash-arbitrage → order-manager
    (Unix socket, ~2μs, process isolation)
```

**Optimize hot paths**: Put frequently-communicating services in the same bundle for Arc transport.

**Isolate critical components**: Put order execution in a separate bundle for fault isolation.

### Smart Transport Selection

The `publisher_to()` and `subscriber_from()` methods automatically select transport:

```rust
// This code works regardless of deployment mode
let pub_ = bus.publisher_to::<SwapEvent>("target-service").await?;

// Transport determined by configuration:
// - Same bundle → AnyPublisher::Local (Arc)
// - Different bundle, same machine → AnyPublisher::Unix
// - Different machine → AnyPublisher::Tcp
```

## Performance

| Scenario | Transport | Latency | Notes |
|----------|-----------|---------|-------|
| Same bundle | Arc | ~50ns | Atomic refcount increment only |
| Different bundle | Unix socket | ~2μs | Syscall overhead, rkyv serialization |
| Different machine | TCP | ~10-100μs | Network latency dependent |

**Zero-copy deserialization**: rkyv allows reading serialized data without reconstruction (2-4x faster than bincode).

## Comparison with Alternatives

### vs ZeroMQ
- **ZeroMQ**: Always serializes, even for in-process communication
- **Mycelium**: Zero-copy Arc for same-bundle, serialization only when needed

### vs Message Brokers (RabbitMQ, Kafka)
- **Brokers**: Separate process, network hop, operational complexity
- **Mycelium**: Direct communication, adaptive transport, embedded

### vs Actor Frameworks (Actix)
- **Actors**: Point-to-point messaging, mailboxes, supervision trees
- **Mycelium**: Pub/sub semantics, simpler model for event streaming

## Project Structure

```
mycelium/
├── crates/
│   ├── mycelium-protocol/    # Message trait, rkyv serialization
│   ├── mycelium-config/       # Topology configuration, bundle resolution
│   └── mycelium-transport/    # MessageBus, Arc/Unix/TCP transports
├── tests/                     # Integration tests
└── examples/                  # Usage examples
```

## Testing

```bash
# Run all tests
cargo test

# Run transport-specific tests
cargo test -p mycelium-transport

# Run integration tests
cargo test --test integration_test
```

**Test coverage**: 35 unit tests, 4 integration tests covering all deployment modes.

## Use Cases

### High-Frequency Trading
- **Development**: Monolith mode for fast iteration (~50ns latency)
- **Production**: Bundled mode with isolated execution (~2μs between bundles)
- **Colocation**: Distributed mode with order execution on colocated server

### Parallel Backtesting
```toml
# Spawn multiple strategy processes, each testing different parameters
[[bundles]]
name = "strategy-1"
services = ["backtest-params-1"]

[[bundles]]
name = "strategy-2"
services = ["backtest-params-2"]
# ... up to strategy-N
```

Each process has isolated state, can run in parallel.

### Horizontal Scaling
Move services to different machines by updating configuration:

```toml
[[bundles]]
name = "risk-monitor"
services = ["risk-engine"]
host = "10.0.1.5"  # Remote machine
port = 9000

# Code unchanged, transport auto-upgrades to TCP
```

## What Mycelium Is NOT

- ❌ **Not an actor framework**: No supervision trees, no actor lifecycle
- ❌ **Not a distributed system framework**: No consensus, no distributed coordination
- ❌ **Not a service mesh**: No sidecar proxies, no traffic management

Mycelium is a **transport abstraction layer** for pub/sub messaging. Keep it simple.

## Roadmap

- [x] Core pub/sub with Arc transport
- [x] Bundle-aware routing
- [x] Unix socket transport
- [x] TCP transport for distributed deployment
- [x] Smart transport selection (`publisher_to`, `subscriber_from`)
- [ ] Health checks and graceful shutdown
- [ ] Metrics and observability
- [ ] Connection pooling and multiplexing
- [ ] TLS for TCP transport

## Contributing

Mycelium is purpose-built for low-latency trading systems but may be useful for other event-driven architectures. Contributions welcome.

## License

MIT OR Apache-2.0

---

**Design Philosophy**: Same code, different deployments. Transport is a configuration concern, not an architectural one.
