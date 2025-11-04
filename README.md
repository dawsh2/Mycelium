# Mycelium <img src="docs/assets/cultures.svg" alt="Mycelium Logo" width="48" style="vertical-align: middle; margin-bottom: -8px;">

Mycelium is a **type-safe pub/sub messaging system** with topology-based transport selection. Define messages once in YAML, write pub/sub code once, deploy anywhere - transport is inferred from configuration. Same binary runs as single-process (Arc), multi-process (Unix sockets), or distributed (TCP). Built for low-latency systems like high-frequency trading, multiplayer game servers, and real-time analytics.

**Key features:** Schema-driven code generation, bijective zerocopy serialization, automatic observability, actor-based supervision with exponential backoff.

---

## Performance (Measured in Release Mode)

**Service API (`ctx.emit()` with full observability):**
- **120 nanoseconds** per emit (includes tracing, metrics, timing)
- **8 million emits/second** per core
- **2.5x overhead** vs raw publish (for full instrumentation)

**Raw Transport Layer:**
- **Same process**: `Arc<T>` zero-copy sharing (47ns) - no serialization
- **Multi-process**: Unix domain sockets (unmeasured, TLV protocol)
- **Distributed**: TCP with zero-copy deserialization (unmeasured, TLV protocol)

*Note: Unix and TCP transport benchmarks pending Phase 3 implementation.*

---

## Quick Start

### 1. Define Your Message Contract

```yaml
# contracts.yaml
messages:
  V2Swap:
    type_id: 19
    domain: market_data
    fields:
      pool_address: "[u8; 20]"
      token0_address: "[u8; 20]"
      token1_address: "[u8; 20]"
      amount0_in: "U256"
      amount0_out: "U256"
      timestamp_ns: "u64"
```

Run `cargo build` - messages are **generated automatically** with the `Message` trait.

### 2. Implement Your Service

```rust
use mycelium::prelude::*;

pub struct PolygonAdapter {
    config: PolygonConfig,  // Your domain config
    cache: MetadataCache,
}

#[mycelium::service]
impl PolygonAdapter {
    async fn run(&mut self, ctx: ServiceContext) -> Result<()> {
        // Connect to data source
        let mut ws = connect_websocket(&self.config.ws_url).await?;

        // Process events
        while let Some(event) = ws.next().await {
            let swap = parse_swap(&event)?;
            
            // Emit typed message - Mycelium handles everything:
            // - TLV serialization (zerocopy)
            // - Routing to subscribers
            // - Transport selection (Arc/Unix/TCP)
            // - Trace context propagation
            // - Metrics collection
            ctx.emit(swap).await?;
        }

        Ok(())
    }
}
```

**What `#[mycelium::service]` does:**
- Wraps your service in an actor with supervision (exponential backoff retry)
- Sets up routing based on message types
- Provides `ServiceContext` with publishers pre-configured
- Adds automatic observability (tracing, metrics, structured logging)

### 3. Wire It Up in Your Binary

```rust
// Your main.rs - YOU control startup
use mycelium_core::{MessageBus, ServiceRuntime};

#[tokio::main]
async fn main() -> Result<()> {
    // Load your config (any format you want)
    let config = load_config("config.toml")?;
    
    // Create your service
    let adapter = PolygonAdapter::new(config);
    
    // Create Mycelium infrastructure
    let bus = MessageBus::new();  // Arc transport (same process)
    let runtime = ServiceRuntime::new(bus);
    
    // Spawn service with supervision
    runtime.spawn_service(adapter).await?;
    
    // Your shutdown logic
    tokio::signal::ctrl_c().await?;
    runtime.shutdown().await?;
    
    Ok(())
}
```

**Start it like any Rust program:**
```bash
cargo run --bin polygon_adapter
# or
./target/release/polygon_adapter
```

### 4. Choose Your Transport

```rust
// Single process (Arc transport - zero copy)
let bus = MessageBus::new();

// Multi-process, same host (Unix sockets)
let bus = MessageBus::with_unix_transport("/tmp/bandit")?;

// Distributed (TCP)
let bus = MessageBus::with_tcp_transport("10.0.1.10:9000")?;
```

**Same service code works with any transport.** Change one line, redeploy.

---

## What `ctx.emit()` Actually Does

When you call `ctx.emit(message).await?`, here's what Mycelium handles automatically:

1. **Timing**: Measures emit latency for metrics (40ns overhead)
2. **Topic Routing**: Maps message type to topic (e.g., `V2Swap` → `"market_data.v2_swaps"`)
3. **Publisher Lookup**: Finds the correct `Publisher` for this message type (40ns DashMap lookup)
4. **Envelope Wrapping**: Wraps message in `Envelope` with type information
5. **Transport**: Sends via `broadcast::channel` (local Arc transport)
6. **Metrics Recording**: Records emit count and latency (2ns atomic operations)
7. **Trace Logging**: Logs emit with trace_id context (1ns when disabled)

**Current implementation** (Phase 2):
- ✅ Local transport (`Arc<Envelope>`) - zero-copy, 47ns baseline
- ✅ Automatic metrics collection (emits_total, emit_latency_us_avg)
- ✅ Trace context in logs (trace_id included automatically)
- ⏳ Wire format serialization (Unix/TCP) - deferred to Phase 2.5
- ⏳ Cross-service trace propagation - deferred to Phase 2.5

**Your code:** `ctx.emit(swap).await?;` (**120ns total**)

**Mycelium does:** Timing → Routing → Envelope → Transport → Metrics → Logging

---

## Architecture

### Core Components

- **`mycelium-protocol`** - Message trait, TYPE_ID, TOPIC, TLV serialization
- **`mycelium-core`** - MessageBus, Publishers, Subscribers, Arc/Unix/TCP transports, configuration
- **`mycelium-service`** - `#[service]` macro, ServiceContext, ServiceRuntime (optional)
- **`mycelium-service-macro`** - Proc macro implementation (internal)

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

- **[docs/implementation/SERVICE-API-DESIGN.md](docs/implementation/SERVICE-API-DESIGN.md)** - Complete service API specification
- **[docs/implementation/LIBRARY-NOT-FRAMEWORK.md](docs/implementation/LIBRARY-NOT-FRAMEWORK.md)** - Why Mycelium is a library, not a framework
- **[docs/implementation/CONFIG-BOUNDARY-ANALYSIS.md](docs/implementation/CONFIG-BOUNDARY-ANALYSIS.md)** - Config ownership (Mycelium vs application)
- **[docs/TRANSPORT.md](docs/TRANSPORT.md)** - Transport architecture, wire protocol, TLV format
- **[docs/ACTORS.md](docs/ACTORS.md)** - Actor system (already implemented)

---

## License

MIT OR Apache-2.0
