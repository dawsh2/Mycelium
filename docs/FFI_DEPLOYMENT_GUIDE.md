# FFI Deployment Guide: In-Process Python Integration

This guide explains how to use the **FFI (Foreign Function Interface) path** for embedding Python code directly in your Rust process, achieving near-zero overhead access to the MessageBus.

## Why FFI?

The FFI path provides the **fastest possible integration** for Python services:

- ✅ **Direct MessageBus access**: Python code uses the same `Arc<MessageBus>` as Rust
- ✅ **Minimal overhead**: ~100-500ns (just the language boundary)
- ✅ **Topology-aware**: Auto-routing to Unix/TCP endpoints
- ✅ **Single binary**: Simplified deployment
- ✅ **Shared memory**: Already using in-process shared state

### Performance Comparison

| Integration Path | Latency | Use Case |
|-----------------|---------|----------|
| **FFI (In-Process)** | ~100-500ns | Trusted Python, maximum performance |
| **Shared Memory (Out-of-Process)** | ~200-500ns | Process isolation + good performance |
| **Unix Socket** | ~1-10μs | Same-machine, process isolation |
| **TCP Socket** | ~100μs-10ms | Distributed, cross-machine |

## Architecture

```
┌────────────────────────────────────────┐
│  Single Process                        │
│  ┌──────────────────────────────────┐  │
│  │ Python Code                      │  │
│  │  import mycelium_native          │  │
│  │  runtime.publish(msg)            │  │
│  └───────────────▲──────────────────┘  │
│                  │ PyO3/C ABI           │
│  ┌───────────────▼──────────────────┐  │
│  │ MyceliumRuntime (FFI)            │  │
│  │  • Arc<MessageBus> ◄─────────────┼──┐
│  │  • Tokio runtime                 │  │  Same Arc!
│  └──────────────────────────────────┘  │  │
│                                         │  │
│  ┌──────────────────────────────────┐  │  │
│  │ Rust Services                    │  │  │
│  │  • Use same MessageBus ◄─────────┼──┘
│  └──────────────────────────────────┘  │
└────────────────────────────────────────┘
```

**Key Insight**: Once across the FFI boundary, Python messages travel through the **same Arc<MessageBus>** as Rust services. There's no separate transport - it's genuine shared memory access.

## Quick Start

### 1. Basic Usage (No Topology)

```python
from mycelium_native import Runtime
from mycelium_protocol import SCHEMA_DIGEST
from mycelium_protocol.messages import TradeSignal

# Create FFI runtime (uses local MessageBus)
runtime = Runtime(schema_digest=SCHEMA_DIGEST)

# Publish directly to the bus
runtime.publish(TradeSignal(symbol="BTC", action="buy"))

# Subscribe with callback
def on_trade(msg):
    print(f"Trade: {msg.symbol} {msg.action}")

subscription = runtime.subscribe(TradeSignal, on_trade)

# Cleanup
subscription.close()
runtime.close()
```

### 2. Topology-Aware Usage (NEW!)

```python
from mycelium_native import Runtime
from mycelium_protocol import SCHEMA_DIGEST

# Create runtime from topology (auto-configures transports)
runtime = Runtime(
    schema_digest=SCHEMA_DIGEST,
    topology_path="topology.toml",
    service_name="python-ml-model"
)

# Now you can publish to remote services!
runtime.publish(TradeSignal(symbol="BTC", action="buy"))
# ↑ Auto-routes to correct endpoint based on topology

# Cleanup
runtime.close()
```

## Deployment Modes

### Mode 1: Monolithic (All in One Process)

**Best for**: Development, latency-critical applications, trusted code

```toml
# topology.toml
[[nodes]]
name = "main"
services = [
    "rust-strategy",      # Rust service
    "python-ml-model",    # Python FFI service
    "rust-executor"       # Rust service
]
```

**Benefits**:
- Lowest latency (~100-500ns)
- Single binary deployment
- All services share the same MessageBus via Arc

**Tradeoffs**:
- Python crash crashes entire process
- GIL contention with Rust async tasks

### Mode 2: Hybrid (FFI + Remote Endpoints)

**Best for**: Production with mixed local/remote services

```toml
# topology.toml
[[nodes]]
name = "ml-node"
services = ["python-ml-model"]  # FFI service
endpoint.kind = "tcp"
endpoint.addr = "0.0.0.0:9091"

[[nodes]]
name = "strategy-node"
services = ["rust-strategy"]
host = "10.0.2.20"
port = 9092
```

```python
# Python FFI service can now route to remote Rust services
runtime = Runtime(
    topology_path="topology.toml",
    service_name="python-ml-model"
)

# Publishes locally (FFI)
runtime.publish(LocalMetric(...))

# Routes to remote service (TCP)
runtime.publish(StrategyCommand(...))
```

**Benefits**:
- Local Python has FFI performance
- Can communicate with remote services
- Process isolation for critical services

### Mode 3: Pure FFI Cluster (All Embedded)

**Best for**: Edge deployments, single-machine clusters

```toml
[[nodes]]
name = "edge-device"
services = [
    "rust-collector",
    "python-preprocessor",  # FFI
    "rust-aggregator",
    "python-alerter"        # FFI
]
```

**Benefits**:
- No network/IPC overhead
- Simplified deployment (single binary)
- Maximum throughput

## Integration Patterns

### Pattern 1: Event Handler

```python
from mycelium_native import Runtime

runtime = Runtime(
    topology_path="topology.toml",
    service_name="event-processor"
)

def handle_market_data(msg):
    # Process message
    result = process(msg)

    # Publish result back to bus
    runtime.publish(ProcessedData(result=result))

runtime.subscribe(MarketData, handle_market_data)

# Keep running
import time
while True:
    time.sleep(1)
```

### Pattern 2: Request-Response

```python
import queue
import threading

runtime = Runtime(topology_path="topology.toml", service_name="ml-service")

# Response queue
responses = queue.Queue()

def handle_request(req):
    # Process ML request
    prediction = model.predict(req.features)
    response = PredictionResponse(request_id=req.id, prediction=prediction)
    runtime.publish(response)

runtime.subscribe(PredictionRequest, handle_request)
```

### Pattern 3: Embedded ML Pipeline

```python
import torch
from mycelium_native import Runtime

runtime = Runtime(topology_path="topology.toml", service_name="ml-pipeline")

# Load model
model = torch.load("model.pt")

def handle_features(msg):
    # Run inference
    with torch.no_grad():
        output = model(msg.features)

    # Publish prediction
    runtime.publish(Prediction(
        symbol=msg.symbol,
        score=output.item()
    ))

runtime.subscribe(Features, handle_features)
```

## Topology Configuration

### Endpoint Types

```toml
[[nodes]]
name = "python-worker"
services = ["ml-model"]

# Option 1: FFI (in-process, no endpoint needed)
# - Just include in services list
# - No endpoint configuration required

# Option 2: Expose via Unix socket (for other processes)
endpoint.kind = "unix"  # Creates /tmp/mycelium/python-worker.sock

# Option 3: Expose via TCP (for remote access)
endpoint.kind = "tcp"
endpoint.addr = "0.0.0.0:9091"

# Option 4: Expose via shared memory (future)
endpoint.kind = "shm"
endpoint.path = "/dev/shm/mycelium/python-worker.shm"
```

### Mixed Deployment Example

```toml
socket_dir = "/tmp/mycelium"

# FFI service with Unix endpoint for other local processes
[[nodes]]
name = "ml-node"
services = ["python-ml-model"]
endpoint.kind = "unix"

# Remote Rust service
[[nodes]]
name = "strategy-node"
services = ["rust-strategy"]
host = "10.0.2.20"
port = 9092

# Another FFI service
[[nodes]]
name = "alerts-node"
services = ["python-alerter"]
endpoint.kind = "tcp"
endpoint.addr = "0.0.0.0:9093"
```

## Performance Tuning

### 1. Minimize GIL Contention

```python
# Bad: Long-running computation in callback
def slow_handler(msg):
    result = expensive_computation(msg)  # Holds GIL
    runtime.publish(Result(result))

# Good: Offload to thread pool
import concurrent.futures
executor = concurrent.futures.ThreadPoolExecutor(max_workers=4)

def fast_handler(msg):
    future = executor.submit(expensive_computation, msg)
    future.add_done_callback(lambda f: runtime.publish(Result(f.result())))
```

### 2. Batch Processing

```python
import queue
import threading

batch_queue = queue.Queue(maxsize=1000)

def batch_worker():
    while True:
        batch = []
        try:
            # Collect up to 100 messages
            for _ in range(100):
                batch.append(batch_queue.get(timeout=0.01))
        except queue.Empty:
            pass

        if batch:
            results = model.predict_batch(batch)
            for result in results:
                runtime.publish(result)

threading.Thread(target=batch_worker, daemon=True).start()

def handle_input(msg):
    batch_queue.put(msg)

runtime.subscribe(Input, handle_input)
```

### 3. Use NumPy/Torch for Zero-Copy

```python
import numpy as np

def handle_array_data(msg):
    # Convert to NumPy array (zero-copy if possible)
    arr = np.frombuffer(msg.data, dtype=np.float32)

    # Process
    result = process(arr)

    # Publish
    runtime.publish(Result(data=result.tobytes()))
```

## Error Handling

### Schema Digest Mismatch

```python
from mycelium_native import Runtime
from mycelium_protocol import SCHEMA_DIGEST

try:
    runtime = Runtime(schema_digest=SCHEMA_DIGEST)
except ValueError as e:
    print(f"Schema mismatch: {e}")
    # Regenerate bindings with: cargo run -p mycelium-codegen
```

### Topology Errors

```python
try:
    runtime = Runtime(
        topology_path="topology.toml",
        service_name="unknown-service"
    )
except RuntimeError as e:
    print(f"Topology error: {e}")
    # Service not found in topology
```

### Graceful Shutdown

```python
import signal
import sys

runtime = Runtime(topology_path="topology.toml", service_name="worker")

def shutdown_handler(sig, frame):
    print("Shutting down...")
    runtime.close()
    sys.exit(0)

signal.signal(signal.SIGINT, shutdown_handler)
signal.signal(signal.SIGTERM, shutdown_handler)
```

## Comparison: FFI vs Socket vs Shared Memory

| Feature | FFI | Shared Memory | Unix Socket | TCP Socket |
|---------|-----|---------------|-------------|------------|
| **Latency** | ~100-500ns | ~200-500ns | ~1-10μs | ~100μs-10ms |
| **Process Isolation** | ❌ | ✅ | ✅ | ✅ |
| **Crash Safety** | ❌ (crashes affect all) | ✅ | ✅ | ✅ |
| **Deployment** | Single binary | Same machine | Same machine | Distributed |
| **Complexity** | Simple | Medium | Simple | Simple |
| **Topology Support** | ✅ | ✅ | ✅ | ✅ |
| **Best For** | Trusted code, max perf | Isolation + speed | Isolation, simple | Distributed |

## When NOT to Use FFI

❌ **Untrusted Python code**: Crashes will take down the entire process
❌ **Heavy CPU-bound tasks**: GIL will block Rust async tasks
❌ **Already distributed**: If services run on different machines anyway
❌ **Need hot reload**: FFI requires process restart to update Python code

**In these cases, use**:
- **Shared Memory Transport**: For isolation + performance
- **Unix/TCP Sockets**: For full isolation and simplicity

## Migration Path

### From Socket Transport to FFI

```python
# Before: Out-of-process socket
from mycelium import UnixTransport, Publisher, Subscriber

transport = UnixTransport("/tmp/mycelium/worker.sock", SCHEMA_DIGEST)
transport.connect()
pub = transport.publisher(Message)
sub = transport.subscriber(Message)

# After: In-process FFI
from mycelium_native import Runtime

runtime = Runtime(
    topology_path="topology.toml",
    service_name="worker"
)
runtime.publish(Message(...))
runtime.subscribe(Message, callback)
```

### From Bare FFI to Topology-Aware

```python
# Before: Bare FFI (local only)
runtime = Runtime(schema_digest=SCHEMA_DIGEST)
runtime.publish(msg)  # Only local bus

# After: Topology-aware FFI (local + remote)
runtime = Runtime(
    schema_digest=SCHEMA_DIGEST,
    topology_path="topology.toml",
    service_name="worker"
)
runtime.publish(msg)  # Auto-routes to correct endpoint
```

## Summary

The **FFI path is the fastest lane** for Python-Rust integration:

✅ **Use FFI when**: Trusted code, single-machine, maximum performance
✅ **Topology support**: Auto-routes to remote endpoints when needed
✅ **True shared memory**: Direct access to Arc<MessageBus>
✅ **~100-500ns latency**: 10-100x faster than sockets

For production systems requiring process isolation, use **Shared Memory Transport** (coming in v0.3.0) or **Unix/TCP sockets**.

See also:
- [Cross-Language Integration Architecture](architecture/CROSS_LANGUAGE_INTEGRATION.md)
- [Shared Memory Transport Plan](implementation/SHARED_MEMORY_TRANSPORT.md)
- [Python Bridge Usage Guide](python-bridge/USAGE.md)
