# Mycelium Deployment Options: Complete Guide

This document provides a comprehensive overview of all deployment options for Mycelium, covering in-process, out-of-process, and distributed scenarios.

## Quick Reference

| Transport | Latency | Isolation | Use Case | Language Support |
|-----------|---------|-----------|----------|------------------|
| **Arc<T>** | ~100ns | None | In-process Rust-only | Rust only |
| **FFI** | ~100-500ns | None | In-process with Python | Rust + Python |
| **Shared Memory** | ~200-500ns | ✅ Process | Same-machine, fast | Rust + Python + OCaml |
| **Unix Socket** | ~1-10μs | ✅ Process | Same-machine, simple | Rust + Python + OCaml |
| **TCP Socket** | ~100μs-10ms | ✅ Process + Network | Distributed | Rust + Python + OCaml |

## Option 1: Arc<T> (Rust-Only, Fastest)

**Performance**: ~100ns round-trip
**Isolation**: None (same process)
**Languages**: Rust only

### When to Use

- Pure Rust services
- Maximum performance required
- Trusted code only
- Single-machine deployment

### Example

```rust
use mycelium_transport::MessageBus;

let bus = MessageBus::new();
let pub = bus.publisher::<TradeSignal>();
let mut sub = bus.subscriber::<TradeSignal>();

// Zero-copy publish
pub.publish(TradeSignal { symbol: "BTC", action: Action::Buy }).await?;

// Zero-copy receive (Arc<TradeSignal>)
let msg = sub.recv().await?;
```

### Topology

```toml
[[nodes]]
name = "main"
services = ["collector", "strategy", "executor"]  # All Rust
```

**Result**: All services use Arc<T>, ~100ns latency between them.

---

## Option 2: FFI (In-Process Python, Fast)

**Performance**: ~100-500ns round-trip
**Isolation**: None (same process, shared Arc<MessageBus>)
**Languages**: Rust + Python

### When to Use

- Embed Python in Rust process
- Need Python ML/data processing
- Trust Python code (won't crash)
- Want maximum Python performance
- **This is the primary path for in-process cross-language integration**

### Architecture

```
Single Process
├── Rust Services ──┐
├── Python FFI  ────┤──► All share same Arc<MessageBus>
└── Rust Services ──┘
```

### Example (Python)

```python
from mycelium_native import Runtime
from mycelium_protocol import SCHEMA_DIGEST

# Option A: Basic (local MessageBus only)
runtime = Runtime(schema_digest=SCHEMA_DIGEST)
runtime.publish(TradeSignal(symbol="BTC", action="buy"))

# Option B: Topology-aware (can route to remote endpoints)
runtime = Runtime(
    schema_digest=SCHEMA_DIGEST,
    topology_path="topology.toml",
    service_name="python-ml-model"
)
runtime.publish(Signal(...))  # Auto-routes based on topology
```

### Topology

```toml
[[nodes]]
name = "main"
services = [
    "rust-collector",     # Rust
    "python-ml-model",    # Python FFI
    "rust-executor"       # Rust
]
# All in same process, share Arc<MessageBus>
```

**Result**: Rust and Python share the same bus, ~100-500ns latency.

### Performance Notes

- **Direct Arc<MessageBus> access**: Python publishes go straight into the same Arc as Rust
- **Serialization still required**: Messages must cross language boundary as bytes
- **GIL overhead**: Python GIL can block Rust async tasks
- **Crash risk**: Python crash takes down entire process

**See**: [FFI Deployment Guide](FFI_DEPLOYMENT_GUIDE.md)

---

## Option 3: Shared Memory (Out-of-Process, Very Fast)

**Performance**: ~200-500ns round-trip
**Isolation**: ✅ Process-isolated
**Languages**: Rust + Python + OCaml
**Status**: ✅ **Implemented (v0.3.0)**

### When to Use

- Need process isolation
- Unix/TCP sockets too slow
- Same-machine deployment
- **Recommended for out-of-process scenarios**

### Architecture

```
Process A                  Process B
┌───────────┐             ┌───────────┐
│ Rust Bus  │             │ Python    │
│    ↓      │             │  Worker   │
│ ShmWriter │→→ /dev/shm →→│ ShmReader │
│ ShmReader │←← (mmap)  ←←│ ShmWriter │
└───────────┘             └───────────┘
```

### Example (Rust)

```rust
use mycelium_transport::{MessageBus, bind_shm_endpoint};

let bus = MessageBus::new();
let handle = bind_shm_endpoint(
    "/dev/shm/mycelium/worker.shm",
    bus.local_transport(),
    SCHEMA_DIGEST,
    None,  // capacity (default: 1MB)
    None,  // stats
).await?;
```

### Example (Python)

```python
from mycelium import SharedMemoryTransport

transport = SharedMemoryTransport.open(
    "/dev/shm/mycelium/worker.shm",
    schema_digest=SCHEMA_DIGEST
)

pub = transport.publisher(TradeSignal)
sub = transport.subscriber(MarketData)
```

### Topology

```toml
[[nodes]]
name = "rust-node"
services = ["strategy"]
endpoint.kind = "shm"
endpoint.path = "/dev/shm/mycelium/rust-node.shm"

[[nodes]]
name = "python-worker"
services = ["ml-model"]
endpoint.kind = "shm"
endpoint.path = "/dev/shm/mycelium/python-worker.shm"
```

**Result**: Separate processes, ~200-500ns latency (10-50x faster than sockets).

### Performance Notes

- **Lock-free ring buffer**: Atomic operations for sync
- **No syscalls**: mmap-backed, no read()/write()
- **Cache-line aligned**: 64-byte header alignment
- **Sequence checking**: Detects corruption

**See**: [Shared Memory Transport Plan](implementation/SHARED_MEMORY_TRANSPORT.md)

---

## Option 4: Unix Sockets (Out-of-Process, Simple)

**Performance**: ~1-10μs round-trip
**Isolation**: ✅ Process-isolated
**Languages**: Rust + Python + OCaml

### When to Use

- Need process isolation
- Same-machine deployment
- Simplicity over performance
- Well-understood infrastructure

### Example (Rust)

```rust
let bus = MessageBus::from_topology("topology.toml", "strategy")?;
// Auto-binds Unix socket based on topology
```

### Example (Python)

```python
from mycelium import MessageBus

bus = MessageBus.from_topology(
    "topology.toml",
    service_name="python-worker",
    schema_digest=SCHEMA_DIGEST
)

# Auto-connects to Unix socket
pub = bus.publisher_to("strategy-service", Command)
```

### Topology

```toml
socket_dir = "/tmp/mycelium"

[[nodes]]
name = "strategy"
services = ["rust-strategy"]
# Creates /tmp/mycelium/strategy.sock

[[nodes]]
name = "worker"
services = ["python-worker"]
# Connects to /tmp/mycelium/strategy.sock
```

**Result**: Separate processes, ~1-10μs latency.

---

## Option 5: TCP Sockets (Distributed)

**Performance**: ~100μs-10ms round-trip (network-dependent)
**Isolation**: ✅ Process + network isolation
**Languages**: Rust + Python + OCaml

### When to Use

- Distributed deployment (multiple machines)
- Kubernetes/container environments
- Geographic distribution
- Cloud deployments

### Example (Rust)

```rust
let bus = MessageBus::from_topology("topology.toml", "strategy")?;
// Auto-connects via TCP based on topology
```

### Example (Python)

```python
bus = MessageBus.from_topology(
    "topology.toml",
    service_name="python-worker",
    schema_digest=SCHEMA_DIGEST
)

pub = bus.publisher_to("remote-service", Command)
```

### Topology

```toml
[[nodes]]
name = "strategy-node"
services = ["rust-strategy"]
host = "10.0.1.10"
port = 9001

[[nodes]]
name = "worker-node"
services = ["python-worker"]
host = "10.0.2.20"
port = 9002
```

**Result**: Separate machines, ~100μs-10ms latency.

---

## Decision Tree

```
┌─────────────────────────────────────────┐
│ Is it all Rust?                         │
└────────┬────────────────────────────────┘
         │
    ┌────┴────┐
    │ YES     │ NO
    │         │
    v         v
┌────────┐  ┌───────────────────────────────┐
│ Arc<T> │  │ Need process isolation?       │
│ ~100ns │  └──────┬────────────────────────┘
└────────┘         │
            ┌──────┴───────┐
            │ YES          │ NO
            │              │
            v              v
    ┌───────────────┐  ┌──────────────┐
    │ Same machine? │  │ Trust Python?│
    └──────┬────────┘  └──────┬───────┘
           │                  │
      ┌────┴────┐        ┌────┴────┐
      │ YES     │ NO     │ YES     │ NO
      │         │        │         │
      v         v        v         v
  ┌───────┐ ┌─────┐  ┌─────┐  ┌──────┐
  │ Shared│ │ TCP │  │ FFI │  │ Unix │
  │ Memory│ │     │  │     │  │Socket│
  │~200ns │ │~1ms │  │500ns│  │~5μs  │
  └───────┘ └─────┘  └─────┘  └──────┘
```

## Performance Spectrum

```
Fastest ←────────────────────────────────────────────→ Slowest
Arc<T>   FFI    Shared   Unix      TCP
~100ns   ~500ns Memory   Socket    Socket
               ~200ns    ~5μs      ~1ms

         ↑      ↑
         │      └─ Process isolation starts here
         │
         └─ Language boundary (Python/OCaml)
```

## Recommended Configurations

### 1. High-Frequency Trading

```toml
[[nodes]]
name = "trading-engine"
services = [
    "market-data",      # Rust
    "python-signals",   # Python FFI
    "execution"         # Rust
]
```

**Transport**: FFI (~500ns)
**Why**: Maximum speed, acceptable crash risk

### 2. Machine Learning Pipeline

```toml
[[nodes]]
name = "ml-node"
services = ["python-model"]
endpoint.kind = "shm"

[[nodes]]
name = "strategy-node"
services = ["rust-strategy"]
endpoint.kind = "shm"
```

**Transport**: Shared Memory (~200ns)
**Why**: Process isolation + great performance

### 3. Microservices (Single Machine)

```toml
socket_dir = "/tmp/mycelium"

[[nodes]]
name = "collector"
services = ["rust-collector"]

[[nodes]]
name = "processor"
services = ["python-processor"]

[[nodes]]
name = "executor"
services = ["rust-executor"]
```

**Transport**: Unix Sockets (~5μs)
**Why**: Simple, process-isolated, good enough

### 4. Distributed System

```toml
[[nodes]]
name = "collector-nyc"
services = ["market-data"]
host = "10.0.1.10"
port = 9001

[[nodes]]
name = "strategy-london"
services = ["trading-strategy"]
host = "10.0.2.20"
port = 9002

[[nodes]]
name = "execution-tokyo"
services = ["order-execution"]
host = "10.0.3.30"
port = 9003
```

**Transport**: TCP Sockets (~1ms+)
**Why**: Distributed requirements, network latency unavoidable

## Language Support Matrix

| Transport | Rust | Python | OCaml | Future (JS, Go) |
|-----------|------|--------|-------|-----------------|
| **Arc<T>** | ✅ | ❌ | ❌ | ❌ |
| **FFI** | ✅ | ✅ | 🟡 Planned | 🟡 Possible |
| **Shared Memory** | ✅ | ✅ | 🟡 Planned | 🟡 Possible |
| **Unix Socket** | ✅ | ✅ | ✅ | ✅ Easy |
| **TCP Socket** | ✅ | ✅ | ✅ | ✅ Easy |

## Migration Paths

### Socket → FFI (For Speed)

**Before**:
```python
from mycelium import UnixTransport
transport = UnixTransport("/tmp/mycelium/worker.sock", DIGEST)
```

**After**:
```python
from mycelium_native import Runtime
runtime = Runtime(topology_path="topology.toml", service_name="worker")
```

**Gain**: 10-20x latency improvement

### FFI → Shared Memory (For Safety)

**Before**:
```python
from mycelium_native import Runtime
runtime = Runtime(schema_digest=DIGEST)
```

**After**:
```python
from mycelium import SharedMemoryTransport
transport = SharedMemoryTransport.open("/dev/shm/mycelium/worker.shm", DIGEST)
```

**Gain**: Process isolation, minimal latency increase

### Socket → Shared Memory (For Speed + Safety)

**Before**:
```python
from mycelium import UnixTransport
transport = UnixTransport("/tmp/mycelium/worker.sock", DIGEST)
```

**After**:
```python
from mycelium import SharedMemoryTransport
transport = SharedMemoryTransport.open("/dev/shm/mycelium/worker.shm", DIGEST)
```

**Gain**: 5-50x latency improvement, same isolation

## Summary

**For maximum flexibility**, use `MessageBus.from_topology()` in all languages:

- **Rust**: Auto-selects Arc/Unix/TCP based on topology
- **Python (FFI)**: Auto-routes to local Arc or remote endpoints
- **Python (Socket)**: Auto-selects Unix/TCP/Shared Memory

**Performance ranking**:
1. **Arc<T>**: ~100ns (Rust-only, in-process)
2. **FFI**: ~100-500ns (Python/Rust in-process, shared Arc)
3. **Shared Memory**: ~200-500ns (out-of-process, same-machine)
4. **Unix Socket**: ~1-10μs (out-of-process, same-machine)
5. **TCP Socket**: ~100μs-10ms (distributed, network-dependent)

**Recommendation**:
- **In-process Python**: Use FFI (fastest)
- **Out-of-process, same-machine**: Use Shared Memory (new standard)
- **Distributed**: Use TCP Sockets (only option)

See also:
- [FFI Deployment Guide](FFI_DEPLOYMENT_GUIDE.md)
- [Cross-Language Integration Architecture](architecture/CROSS_LANGUAGE_INTEGRATION.md)
- [Shared Memory Transport Implementation](implementation/SHARED_MEMORY_TRANSPORT.md)
