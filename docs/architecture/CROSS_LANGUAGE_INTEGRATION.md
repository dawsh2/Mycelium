# Cross-Language Integration Architecture

This document explains the architecture decisions, tradeoffs, and deployment options for integrating non-Rust languages with Mycelium.

## Table of Contents

- [Overview](#overview)
- [Integration Paths](#integration-paths)
- [Transport Comparison](#transport-comparison)
- [Deployment Flexibility](#deployment-flexibility)
- [Decision Tree](#decision-tree)
- [Performance Expectations](#performance-expectations)
- [Future: Shared Memory Transport](#future-shared-memory-transport)

## Overview

Mycelium provides **three integration paths** for non-Rust languages:

1. **Socket Bridges** (Out-of-Process): Run non-Rust services as separate processes
2. **Native FFI** (In-Process): Embed non-Rust code directly in Rust processes
3. **Hybrid**: Combine both approaches in a single deployment

Each path has specific tradeoffs around **safety**, **performance**, and **deployment flexibility**.

## Integration Paths

### Path 1: Socket Bridges (Recommended for Most Use Cases)

**Architecture**:
```
┌─────────────────────────┐
│  Rust Process           │
│  ┌──────────────────┐   │
│  │ MessageBus       │   │
│  │ (Arc<T>)         │   │
│  │       ↓          │   │
│  │ PythonBridge     │◄──┼─── Unix Socket ────┐
│  │ Service          │   │                     │
│  └──────────────────┘   │                     │
└─────────────────────────┘                     │
                                                │
┌─────────────────────────┐                     │
│  Python Process         │                     │
│  ┌──────────────────┐   │                     │
│  │ MessageBus       │◄──┼─────────────────────┘
│  │ (from_topology)  │   │
│  └──────────────────┘   │
└─────────────────────────┘
```

**When to Use**:
- Production deployments requiring process isolation
- Services that may crash and need independent restart
- Mixed-language systems where each language has its own lifecycle
- Distributed deployments across multiple machines

**Pros**:
- ✅ **Process isolation**: Python crashes don't affect Rust
- ✅ **Independent deployment**: Update workers without restarting bus
- ✅ **Language runtime safety**: No FFI complexity
- ✅ **Supports Unix and TCP**: Same-machine or distributed
- ✅ **Topology-aware routing**: Use `MessageBus.from_topology()` for auto-routing

**Cons**:
- ❌ **IPC overhead**: Socket serialization (~1-10μs latency)
- ❌ **Cannot use Arc<T>**: Must serialize to TLV format
- ❌ **Separate processes**: Requires process management

**Code Example** (Python):
```python
from mycelium import MessageBus

# Load topology and auto-configure transports
bus = MessageBus.from_topology(
    "topology.toml",
    service_name="python-worker",
    schema_digest=SCHEMA_DIGEST
)

# Auto-routes to target service (Unix or TCP based on topology)
pub = bus.publisher_to("strategy-service", TradeSignal)
pub.publish(TradeSignal(symbol="BTC", action="buy"))

# Subscribe to local bridge
sub = bus.subscriber(MarketData)
for msg in sub:
    process(msg)
```

### Path 2: Native FFI (For Low-Latency Embedded Use Cases)

**Architecture**:
```
┌───────────────────────────────────────┐
│  Rust Process                         │
│  ┌────────────────────────────────┐   │
│  │ MyceliumRuntime (FFI)          │   │
│  │  • MessageBus                  │   │
│  │  • Tokio runtime               │   │
│  └────────────▲───────────────────┘   │
│               │ C ABI                 │
│  ┌────────────▼───────────────────┐   │
│  │ mycelium_native (PyO3)         │   │
│  └────────────▲───────────────────┘   │
│               │ CPython API           │
│  ┌────────────▼───────────────────┐   │
│  │ Python Code                    │   │
│  │  import mycelium_native        │   │
│  └────────────────────────────────┘   │
└───────────────────────────────────────┘
```

**When to Use**:
- Embedded Python scripts in Rust applications
- Low-latency requirements (avoid socket overhead)
- Monolithic deployments (single binary)
- Trusted Python code that won't crash

**Pros**:
- ✅ **Lower latency**: No IPC overhead (~100-500ns vs ~1-10μs)
- ✅ **Single binary**: Simplified deployment
- ✅ **Shared memory space**: Direct FFI calls

**Cons**:
- ❌ **No process isolation**: Python crashes can crash entire process
- ❌ **GIL contention**: Python GIL can block Rust async tasks
- ❌ **Still requires serialization**: Cannot use Arc<T> zero-copy
- ❌ **Complex FFI safety**: Requires careful memory management

**Code Example** (Python):
```python
from mycelium_native import Runtime

runtime = Runtime(schema_digest=SCHEMA_DIGEST)

# Publish locally
runtime.publish(TradeSignal(symbol="BTC", action="buy"))

# Subscribe with callback
def on_message(msg):
    print(f"Received: {msg}")

subscription = runtime.subscribe(MarketData, on_message)
```

### Path 3: Hybrid (Best of Both Worlds)

Combine FFI for local, low-latency operations with socket bridges for remote communication.

**Example**:
```python
from mycelium_native import Runtime
from mycelium import TcpTransport

# Use FFI for local bus access
local_runtime = Runtime()
local_runtime.publish(LocalEvent(...))

# Use socket for remote service
remote_transport = TcpTransport("10.0.1.10", 9091, SCHEMA_DIGEST)
remote_transport.connect()
remote_pub = remote_transport.publisher(RemoteCommand)
```

## Transport Comparison

### Available Transports by Language

| Transport | Rust | Python (Socket) | Python (FFI) | OCaml |
|-----------|------|-----------------|--------------|-------|
| **Arc<T> (Zero-Copy)** | ✅ | ❌ | ❌ | ❌ |
| **Unix Socket** | ✅ | ✅ | N/A | ✅ |
| **TCP Socket** | ✅ | ✅ | N/A | ✅ |
| **Shared Memory** | 🟡 Future | 🟡 Future | 🟡 Future | 🟡 Future |

### Why Arc<T> is Impossible for Non-Rust Languages

**Arc<T> zero-copy transport** shares Rust trait objects (`Arc<dyn Message>`) directly in memory:

```rust
// Rust: Zero-copy via Arc
let msg = Arc::new(TradeSignal { ... });
bus.publish_arc(msg);  // No serialization!
```

This is **fundamentally impossible** across language boundaries because:

1. **Different memory models**: Python uses garbage collection, Rust uses ownership
2. **No shared type system**: Python objects ≠ Rust trait objects
3. **Safety requirements**: Cannot share Rust memory with Python GC

**However**, you CAN use shared memory with TLV serialization (see [Future: Shared Memory Transport](#future-shared-memory-transport)).

## Deployment Flexibility

### Rust Services (Full Flexibility)

Rust services can use **topology-aware auto-routing**:

```toml
# topology.toml
[[nodes]]
name = "collectors"
services = ["polygon-adapter", "ethereum-adapter"]

[[nodes]]
name = "strategies"
services = ["arbitrage-strategy"]
host = "10.0.2.20"
port = 9001
```

```rust
// Rust: Automatic transport selection
let bus = MessageBus::from_topology("topology.toml")?;
let pub = bus.publisher_to("arbitrage-strategy")?;
// ↑ Uses Unix if same-host, TCP if different-host
```

### Python Services (Topology-Aware as of v0.2.0+)

Python services now support the same topology-aware routing:

```python
# Python: Same API as Rust!
bus = MessageBus.from_topology(
    "topology.toml",
    service_name="python-worker",
    schema_digest=SCHEMA_DIGEST
)
pub = bus.publisher_to("arbitrage-strategy", Command)
# ↑ Auto-selects Unix or TCP based on topology
```

### Deployment Modes

| Mode | Rust | Python (Socket) | Python (FFI) |
|------|------|-----------------|--------------|
| **Monolith** (one process) | ✅ | ❌ | ✅ |
| **Bundled** (multiple processes, one machine) | ✅ | ✅ | N/A |
| **Distributed** (multiple machines) | ✅ | ✅ | N/A |
| **Hybrid** (mix local + remote) | ✅ | ✅ | 🟡 Partial |
| **Topology-aware routing** | ✅ | ✅ | ❌ |

## Decision Tree

```
┌─────────────────────────────────────┐
│ Do you need process isolation?      │
└───────────┬─────────────────────────┘
            │
    ┌───────┴────────┐
    │ YES            │ NO
    │                │
    v                v
┌───────────────┐  ┌────────────────────┐
│ Socket Bridge │  │ Can Python crash?  │
│ (Recommended) │  └─────────┬──────────┘
└───────────────┘            │
                     ┌───────┴────────┐
                     │ YES            │ NO
                     │                │
                     v                v
                 ┌───────────────┐  ┌─────────────┐
                 │ Socket Bridge │  │ Native FFI  │
                 └───────────────┘  └─────────────┘

┌─────────────────────────────────────┐
│ Where is the target service?        │
└───────────┬─────────────────────────┘
            │
    ┌───────┴────────┐
    │ Same machine   │ Different machine
    │                │
    v                v
┌───────────────┐  ┌───────────────┐
│ Unix Socket   │  │ TCP Socket    │
│ (preferred)   │  │ (required)    │
└───────────────┘  └───────────────┘
```

### Quick Recommendations

- **Production systems**: Use socket bridges with `MessageBus.from_topology()`
- **Embedded scripts**: Use native FFI if latency-critical
- **Distributed systems**: Use TCP transport via topology
- **Same-machine services**: Use Unix sockets (automatically selected by topology)
- **Mixed deployment**: Use `MessageBus.from_topology()` for automatic routing

## Performance Expectations

### Latency by Transport

| Transport | Typical Latency | Use Case |
|-----------|----------------|----------|
| **Arc<T>** (Rust only) | ~100ns | In-process, zero-copy |
| **Native FFI** | ~100-500ns | Embedded Python, serialization required |
| **Unix Socket** | ~1-10μs | Same-machine IPC |
| **TCP (localhost)** | ~10-50μs | Localhost network |
| **TCP (network)** | ~100μs-10ms | Cross-machine, depends on network |
| **Shared Memory** (future) | ~100-500ns | Same-machine, no syscalls |

### Throughput

- **Arc<T>**: Millions of messages/second
- **Unix Socket**: Hundreds of thousands of messages/second
- **TCP**: Tens of thousands of messages/second (network-bound)

### Cost Breakdown

For a typical message (100 bytes payload):

```
Arc<T> (Rust):
  - Serialization: 0ns (no serialization)
  - Transport: ~100ns (Arc clone)
  - Deserialization: 0ns
  Total: ~100ns

Unix Socket (Python ↔ Rust):
  - Serialization: ~200ns (TLV encoding)
  - Transport: ~1-5μs (socket write + read)
  - Deserialization: ~200ns (TLV decoding)
  Total: ~1-10μs

TCP Socket (distributed):
  - Serialization: ~200ns
  - Transport: ~100μs-10ms (network latency)
  - Deserialization: ~200ns
  Total: ~100μs-10ms
```

## Future: Shared Memory Transport

While **Arc<T> zero-copy** is impossible across languages, **shared memory with TLV serialization** is possible and would provide much better performance than sockets.

### Proposed Architecture

```
┌────────────────────────────────────────┐
│  Shared Memory Region (mmap)           │
│  ┌──────────────────────────────────┐  │
│  │ Ring Buffer (TLV-encoded frames) │  │
│  └───▲──────────────────────▼───────┘  │
└──────┼────────────────────┼────────────┘
       │                    │
       │                    │
┌──────┴────────┐    ┌──────┴────────┐
│ Rust Process  │    │ Python Process│
│ (Writer)      │    │ (Reader)      │
└───────────────┘    └───────────────┘
```

### Performance Benefits

- **No syscalls**: No `sendto()` / `recvfrom()` overhead
- **No socket buffering**: Direct memory access
- **Still safe**: Each process has its own address space
- **Estimated latency**: ~100-500ns (vs ~1-10μs for Unix sockets)

### Implementation Requirements

1. **Rust side**: Create `SharedMemoryTransport` using `memmap2` crate
2. **Python side**: Use `mmap` module to map the same region
3. **Ring buffer**: Lock-free SPSC or MPSC queue for TLV frames
4. **Synchronization**: Use atomic operations for reader/writer indices

### Example API (Proposed)

```rust
// Rust: Create shared memory transport
let transport = SharedMemoryTransport::new("/dev/shm/mycelium/bus")?;
bus.bind_shared_memory_endpoint(transport)?;
```

```python
# Python: Connect to shared memory
from mycelium import SharedMemoryTransport

transport = SharedMemoryTransport("/dev/shm/mycelium/bus", SCHEMA_DIGEST)
transport.connect()
pub = transport.publisher(Message)
```

### Tradeoffs

**Pros**:
- ✅ Much faster than Unix sockets (~10-100x improvement)
- ✅ Still language-agnostic (works with any language that supports mmap)
- ✅ Process isolation maintained

**Cons**:
- ❌ Still requires TLV serialization (no true zero-copy like Arc<T>)
- ❌ More complex than sockets (ring buffer management)
- ❌ Limited to same-machine deployment
- ❌ Requires careful synchronization (atomic operations, memory barriers)

### Recommendation

This would be a **high-value addition** for Python/OCaml services that:
- Run on the same machine as Rust services
- Need low latency (< 1μs)
- Can't use native FFI due to safety requirements

**Priority**: High for v0.3.0 roadmap

## Summary

### Deployment Parity Scorecard

| Capability | Rust | Python (v0.2.0+) | Python (FFI) | OCaml |
|------------|------|------------------|--------------|-------|
| **Monolith (Arc<T>)** | ✅ | ❌ | 🟡 (still serialized) | ❌ |
| **Bundled (Unix)** | ✅ | ✅ | N/A | ✅ |
| **Distributed (TCP)** | ✅ | ✅ | N/A | ✅ |
| **Topology-aware routing** | ✅ | ✅ (NEW!) | ❌ | 🟡 Planned |
| **Hybrid modes** | ✅ | ✅ | 🟡 Partial | ❌ |
| **Shared memory** | 🟡 Future | 🟡 Future | N/A | 🟡 Future |

### Key Takeaways

1. **Arc<T> is impossible** across language boundaries due to memory model differences
2. **Shared memory with TLV** is possible and would be much faster than sockets
3. **Topology-aware routing** (NEW in v0.2.0) closes the flexibility gap for Python
4. **Socket bridges** are the safest, most flexible option for production
5. **Native FFI** is appropriate for trusted, latency-critical embedded scripts
6. **~60-80% deployment parity** achieved for non-Rust languages (was ~60%, now ~80% with topology support)

The architecture makes **appropriate tradeoffs** for safety and simplicity while providing powerful cross-language integration capabilities.
