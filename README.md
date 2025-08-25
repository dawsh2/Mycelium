## Mycelium

Mycelium is an actor runtime implemented in Rust that emphasizes low-latency, type-safe message passing.  
It provides a consistent API for actors across different deployment topologies.  

Actors always communicate using messages, but the underlying transport adapts depending on where those actors run:

- **Single process**: messages are passed as `Arc<T>` references with no serialization.  
- **Same machine, multiple processes**: messages are serialized once and sent over Unix domain sockets.  
- **Across machines**: messages are serialized and transmitted over TCP.  

This approach allows the same actor code to run efficiently as either a monolith or as part of a distributed system, without requiring changes in application logic.

This approach allows the same actor code to run efficiently as either a monolith or as part of a distributed system, without requiring changes in application logic.  

## Comparison with Related Tools

**ZeroMQ**  
ZeroMQ is a message queue and transport library. It provides sockets and patterns for message passing across processes and networks, but it always requires serialization and copying. Mycelium differs in that same-process actors can share data directly (`Arc<T>`), avoiding serialization overhead.  

**Actix**  
Actix is a Rust actor framework focused on concurrency within a single process. Mycelium provides a similar actor API, but adds adaptive transport so the same code can run across processes or machines without changes.  

**Erlang/OTP**  
Erlang/OTP pioneered the actor model for distributed systems. Mycelium has a similar programming model, but is implemented in Rust and designed for low-latency applications with more control over serialization formats and transports.  

## Example Actor Configuration (TOML)

```toml
# Actors
[[actors]]
id = "market_collector"
type = "Producer"
outputs = ["market_data"]
state_type = "Persistent"
storage = { path = "/var/data/market_collector", sync = true }
checkpoint_interval = "30s"
resources = { memory_mb = 2048, cpu_cores = 2 }

[[actors]]
id = "arbitrage_engine"
type = "Transformer"
inputs = ["market_data"]
outputs = ["signals"]
state_type = "Replicated"
replication_factor = 2
resources = { memory_mb = 4096, cpu_cores = 4 }

# Nodes
[[nodes]]
hostname = "node_01"
numa_nodes = [0, 1]
actor_placements = { market_collector = { numa = 0, cpu = [0,1] },
                     arbitrage_engine = { numa = 1, cpu = [2,3] } }

# Inter-node routes
[[routes]]
source = "node_01"
target = "node_02"
channels = ["market_data"]
protocol = "TCP"
bandwidth_mbps = 1000
latency_ms = 1.0
```
