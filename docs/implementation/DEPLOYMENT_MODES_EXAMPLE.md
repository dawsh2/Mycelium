# Mycelium Deployment Modes: Complete Example

This document shows how an application (trading system) can use Mycelium's different transport modes, from distributed TCP all the way to compile-time routing, **with the same service code**.

---

## The Services (Same Code for All Modes)

```rust
// src/services/arbitrage.rs
use mycelium::prelude::*;

// Service implements handlers for messages it cares about
pub struct ArbitrageService {
    opportunities: Vec<ArbitrageSignal>,
    risk_limits: RiskLimits,
}

// Option 1: MessageHandler trait (for compile-time routing)
impl MessageHandler<V2Swap> for ArbitrageService {
    fn handle(&mut self, swap: &V2Swap) {
        if let Some(signal) = self.find_opportunity(swap) {
            self.opportunities.push(signal);
        }
    }
}

// Option 2: Service macro (for runtime routing with MessageBus)
#[mycelium::service]
impl ArbitrageService {
    async fn run(&mut self, ctx: ServiceContext) -> Result<()> {
        while let Some(msg) = ctx.recv::<V2Swap>().await? {
            self.handle(&msg); // Same business logic!
        }
        Ok(())
    }
}
```

```rust
// src/services/risk.rs
pub struct RiskManager {
    exposure: HashMap<Address, U256>,
    limits: RiskLimits,
}

impl MessageHandler<V2Swap> for RiskManager {
    fn handle(&mut self, swap: &V2Swap) {
        self.update_exposure(swap);
        if self.check_limits().is_err() {
            // Log risk breach
        }
    }
}

#[mycelium::service]
impl RiskManager {
    async fn run(&mut self, ctx: ServiceContext) -> Result<()> {
        while let Some(msg) = ctx.recv::<V2Swap>().await? {
            self.handle(&msg);
        }
        Ok(())
    }
}
```

```rust
// src/services/execution.rs
pub struct ExecutionService {
    pending_orders: Vec<Order>,
}

impl MessageHandler<ArbitrageSignal> for ExecutionService {
    fn handle(&mut self, signal: &ArbitrageSignal) {
        let order = self.create_order(signal);
        self.pending_orders.push(order);
    }
}

#[mycelium::service]
impl ExecutionService {
    async fn run(&mut self, ctx: ServiceContext) -> Result<()> {
        while let Some(msg) = ctx.recv::<ArbitrageSignal>().await? {
            self.handle(&msg);
        }
        Ok(())
    }
}
```

**Key insight:** Services implement business logic once, works in all deployment modes.

---

## Mode 1: Distributed TCP (Multi-Node)

**Use case:** Production distributed system, services on different machines.

```rust
// Node 1: Data ingestion node (receives blockchain swaps)
// main_ingestion.rs

use mycelium::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Connect to message bus via TCP
    let bus = MessageBus::with_tcp("message-broker:9000").await?;
    
    let runtime = ServiceRuntime::new(bus);
    
    // This node only runs data ingestion
    runtime.spawn_service(BlockchainListener::new()).await?;
    
    // BlockchainListener receives swaps and emits V2Swap messages
    // They automatically get routed over TCP to subscribers
    
    runtime.run().await?;
    Ok(())
}
```

```rust
// Node 2: Trading logic node (arbitrage + risk)
// main_trading.rs

use mycelium::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Connect to same message bus
    let bus = MessageBus::with_tcp("message-broker:9000").await?;
    
    let runtime = ServiceRuntime::new(bus);
    
    // This node runs trading services
    runtime.spawn_service(ArbitrageService::new()).await?;
    runtime.spawn_service(RiskManager::new()).await?;
    
    // These services automatically subscribe to V2Swap messages
    // Messages come from Node 1 over TCP
    
    runtime.run().await?;
    Ok(())
}
```

```rust
// Node 3: Execution node (sends orders to DEX)
// main_execution.rs

use mycelium::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    let bus = MessageBus::with_tcp("message-broker:9000").await?;
    
    let runtime = ServiceRuntime::new(bus);
    
    // This node only executes orders
    runtime.spawn_service(ExecutionService::new()).await?;
    
    // Subscribes to ArbitrageSignal messages from Node 2
    
    runtime.run().await?;
    Ok(())
}
```

**Latency:** ~10,000ns per message (TCP network overhead)
**When to use:** Need horizontal scaling, fault isolation, or running on different machines

---

## Mode 2: Unix Domain Sockets (Same Machine, Different Processes)

**Use case:** All services on same machine but separate processes for isolation.

```rust
// Process 1: Data ingestion
// main_ingestion.rs

use mycelium::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Use Unix domain socket instead of TCP
    let bus = MessageBus::with_unix("/tmp/mycelium.sock").await?;
    
    let runtime = ServiceRuntime::new(bus);
    runtime.spawn_service(BlockchainListener::new()).await?;
    
    runtime.run().await?;
    Ok(())
}
```

```rust
// Process 2: Trading + Execution (combined)
// main_trading.rs

use mycelium::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Connect to same Unix socket
    let bus = MessageBus::with_unix("/tmp/mycelium.sock").await?;
    
    let runtime = ServiceRuntime::new(bus);
    
    runtime.spawn_service(ArbitrageService::new()).await?;
    runtime.spawn_service(RiskManager::new()).await?;
    runtime.spawn_service(ExecutionService::new()).await?;
    
    runtime.run().await?;
    Ok(())
}
```

**Latency:** ~1,000ns per message (Unix socket overhead, no network)
**When to use:** Need process isolation but all on same machine (easier debugging, crash isolation)

**No service code changes!** Just changed `with_tcp()` to `with_unix()` in main.rs.

---

## Mode 3: Arc<T> In-Process (Single Process, Separate Async Tasks)

**Use case:** Development, testing, or production monolith with acceptable latency.

```rust
// Single process, multiple async tasks
// main.rs

use mycelium::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Default: Arc-based in-process message bus
    let bus = MessageBus::new(); // or MessageBus::with_arc()
    
    let runtime = ServiceRuntime::new(bus);
    
    // Spawn all services in same process
    runtime.spawn_service(BlockchainListener::new()).await?;
    runtime.spawn_service(ArbitrageService::new()).await?;
    runtime.spawn_service(RiskManager::new()).await?;
    runtime.spawn_service(ExecutionService::new()).await?;
    
    // Each service runs as separate async task
    // Messages passed via Arc<T> + channels (no serialization)
    
    runtime.run().await?;
    Ok(())
}
```

**Latency:** ~65ns per message (Arc clone + channel send)
**When to use:** 
- Development (easy debugging, single process)
- Testing (can mock services easily)
- Production if latency budget >10μs

**Still no service code changes!** Services don't know they're in same process.

---

## Mode 4: Compile-Time Routing (Zero-Cost Abstraction)

**Use case:** Production monolith with ultra low-latency requirements.

### Step 1: Define routing configuration

```rust
// src/routing.rs

use mycelium::routing_config;

routing_config! {
    name: TradingSystem,
    routes: {
        // V2Swap goes to arbitrage and risk services
        V2Swap => [ArbitrageService, RiskManager],
        
        // ArbitrageSignal goes to execution service
        ArbitrageSignal => [ExecutionService],
    }
}

// This macro generates:
//
// pub struct TradingSystem {
//     arbitrage: ArbitrageService,
//     risk: RiskManager,
//     execution: ExecutionService,
// }
//
// impl TradingSystem {
//     #[inline(always)]
//     pub fn route_v2_swap(&mut self, msg: &V2Swap) {
//         self.arbitrage.handle(msg);  // Direct call
//         self.risk.handle(msg);       // Direct call
//     }
//
//     #[inline(always)]
//     pub fn route_arbitrage_signal(&mut self, msg: &ArbitrageSignal) {
//         self.execution.handle(msg);  // Direct call
//     }
// }
```

### Step 2: Use in main.rs

```rust
// main.rs

mod routing;
use routing::TradingSystem;

fn main() -> Result<()> {
    // Create the system with all services
    let mut system = TradingSystem::new(
        ArbitrageService::new(),
        RiskManager::new(),
        ExecutionService::new(),
    );
    
    // Connect to blockchain data source
    let mut blockchain = BlockchainListener::new();
    
    // Main event loop
    loop {
        // Receive swap from blockchain
        let swap = blockchain.next_swap()?;
        
        // Route to handlers - direct function calls!
        system.route_v2_swap(&swap);  // 4ns (2 handlers × 2ns)
        
        // Check if arbitrage service found opportunities
        for signal in system.arbitrage.opportunities.drain(..) {
            system.route_arbitrage_signal(&signal);  // 2ns
        }
        
        // Check if execution service has orders ready
        for order in system.execution.pending_orders.drain(..) {
            // Submit to DEX
            submit_order(order)?;
        }
    }
}
```

**Latency:** ~2-3ns per handler (direct function call)
**When to use:**
- Production single-process deployment
- Ultra low-latency requirements (<1μs)
- High throughput (>1M messages/sec)

**Service code unchanged!** Same `MessageHandler` implementation used by compile-time routing.

---

## Comparison Summary

| Mode | Transport | Latency/msg | Use Case | Service Code Changes |
|------|-----------|-------------|----------|---------------------|
| **TCP** | Network | 10,000ns | Multi-node, distributed | None |
| **Unix Socket** | IPC | 1,000ns | Multi-process, same machine | None (just change main.rs) |
| **Arc<T>** | In-process | 65ns | Dev/test, single process | None (just change main.rs) |
| **Compile-time** | Direct calls | 2-3ns | Production monolith | None (services implement same traits) |

---

## Migration Path: TCP → Unix → Arc → Compile-Time

Let's say you start with TCP and want to optimize:

### Starting Point: Distributed TCP

```rust
// 3 nodes, TCP transport
// Node 1: main_ingestion.rs
let bus = MessageBus::with_tcp("broker:9000").await?;

// Node 2: main_trading.rs  
let bus = MessageBus::with_tcp("broker:9000").await?;

// Node 3: main_execution.rs
let bus = MessageBus::with_tcp("broker:9000").await?;
```

**Latency:** 10μs per message  
**Overhead:** Acceptable for distributed system

---

### Step 1: Consolidate to same machine → Unix sockets

**Decision:** Trading and execution can run on same machine.

```bash
# Deploy ingestion to server-1
# Deploy trading+execution to server-2 (same machine, different processes)
```

```rust
// server-2/process-1: main_trading.rs
let bus = MessageBus::with_unix("/tmp/mycelium.sock").await?;
runtime.spawn_service(ArbitrageService::new()).await?;
runtime.spawn_service(RiskManager::new()).await?;

// server-2/process-2: main_execution.rs
let bus = MessageBus::with_unix("/tmp/mycelium.sock").await?;
runtime.spawn_service(ExecutionService::new()).await?;
```

**Latency:** 1μs per message (10x faster)  
**Code changes:** Just `with_tcp()` → `with_unix()` in main.rs

---

### Step 2: Merge processes → Arc<T>

**Decision:** Process isolation not needed, combine into single process.

```rust
// server-2: main.rs (single process now)
let bus = MessageBus::new();  // Arc transport
let runtime = ServiceRuntime::new(bus);

runtime.spawn_service(ArbitrageService::new()).await?;
runtime.spawn_service(RiskManager::new()).await?;
runtime.spawn_service(ExecutionService::new()).await?;

runtime.run().await?;
```

**Latency:** 65ns per message (15x faster than Unix)  
**Code changes:** Just `with_unix()` → `new()` in main.rs

---

### Step 3: Critical path optimization → Compile-time routing

**Decision:** Order execution is latency-critical, need <1μs end-to-end.

```rust
// src/routing.rs - NEW FILE
routing_config! {
    name: CriticalPath,
    routes: {
        V2Swap => [ArbitrageService, RiskManager],
        ArbitrageSignal => [ExecutionService],
    }
}
```

```rust
// main.rs
use routing::CriticalPath;

fn main() -> Result<()> {
    let mut system = CriticalPath::new(
        ArbitrageService::new(),
        RiskManager::new(),
        ExecutionService::new(),
    );
    
    // Tight event loop - no async overhead
    loop {
        let swap = receive_swap()?;
        system.route_v2_swap(&swap);  // 4ns
        
        for signal in system.arbitrage.opportunities.drain(..) {
            system.route_arbitrage_signal(&signal);  // 2ns
        }
        
        for order in system.execution.pending_orders.drain(..) {
            submit_order(order)?;
        }
    }
}
```

**Latency:** 2-3ns per message (30x faster than Arc)  
**Code changes:** 
- Add `routing.rs` with routing config
- Refactor main loop to be synchronous
- Services unchanged!

---

## Hybrid Approach: Mix and Match

You can also use different modes for different parts of your system:

```rust
// main.rs

fn main() -> Result<()> {
    // Critical path: Compile-time routing (ultra fast)
    let mut critical_path = CriticalPath::new(
        ArbitrageService::new(),
        RiskManager::new(),
        ExecutionService::new(),
    );
    
    // Infrastructure: Runtime routing (flexibility)
    let infra_bus = MessageBus::with_tcp("metrics-collector:9000").await?;
    let infra_runtime = ServiceRuntime::new(infra_bus);
    
    tokio::spawn(async move {
        infra_runtime.spawn_service(MetricsCollector::new()).await?;
        infra_runtime.spawn_service(DatabaseLogger::new()).await?;
        infra_runtime.run().await
    });
    
    // Main loop: Critical path
    loop {
        let swap = receive_swap()?;
        
        // Fast path: direct calls
        critical_path.route_v2_swap(&swap);
        
        // Slow path: emit to infrastructure (async, cloned)
        tokio::spawn({
            let swap = swap.clone();
            let bus = infra_bus.clone();
            async move {
                bus.publish(swap).await
            }
        });
    }
}
```

**Benefit:** 
- Critical path: 2-3ns (compile-time routing)
- Infrastructure: Flexible, can distribute later
- Best of both worlds!

---

## Key Takeaways

1. **Same service code works everywhere**: Services implement `MessageHandler<M>` trait and business logic once.

2. **Transport is a deployment decision**: Change only main.rs to switch between TCP, Unix, Arc, or compile-time.

3. **Progressive optimization**: Start with TCP (distributed), consolidate to Unix (same machine), merge to Arc (same process), optimize to compile-time (direct calls).

4. **No breaking changes**: Each step is opt-in, previous modes still work.

5. **Latency ladder**: 
   - 10,000ns (TCP) → 1,000ns (Unix) → 65ns (Arc) → 2ns (compile-time)
   - Each step is 10-30x faster

6. **Pick the right tool**: Use MessageBus for flexibility, compile-time routing for performance.
