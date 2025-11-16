# Mycelium Architecture Diagrams

Visual guide to Mycelium's architecture, deployment modes, and performance characteristics.

## Table of Contents

1. [Core Architecture](#core-architecture)
2. [Deployment Flexibility](#deployment-flexibility)
3. [Transport Layer](#transport-layer)
4. [Message Flow](#message-flow)
5. [Parallel Backtesting](#parallel-backtesting)
6. [Performance Spectrum](#performance-spectrum)
7. [Cross-Language Integration](#cross-language-integration)

---

## Core Architecture

### High-Level Overview

```mermaid
graph TB
    subgraph "Mycelium Core"
        Bus[MessageBus<br/>Arc&lt;T&gt; Pub/Sub]
        Protocol[Protocol Layer<br/>TLV Encoding]
        Transport[Transport Layer<br/>Local/Socket/Shm]
    end

    subgraph "Services (Rust)"
        S1[Service A]
        S2[Service B]
        S3[Service C]
    end

    subgraph "Services (Python)"
        P1[Python Service<br/>FFI or Socket]
        P2[ML Model<br/>FFI or Socket]
    end

    S1 --> Bus
    S2 --> Bus
    S3 --> Bus
    P1 --> Bus
    P2 --> Bus

    Bus --> Protocol
    Protocol --> Transport

    style Bus fill:#4a9eff
    style Protocol fill:#50c878
    style Transport fill:#ff6b6b
```

### Actor Model

```mermaid
graph LR
    subgraph "Actor System"
        A1[Actor 1<br/>State]
        A2[Actor 2<br/>State]
        A3[Actor 3<br/>State]
    end

    subgraph "MessageBus"
        Pub[Publishers]
        Sub[Subscribers]
        Chan[Channels]
    end

    A1 -->|Publish| Pub
    A2 -->|Publish| Pub
    Pub --> Chan
    Chan --> Sub
    Sub -->|Deliver| A1
    Sub -->|Deliver| A2
    Sub -->|Deliver| A3

    style A1 fill:#ffd93d
    style A2 fill:#ffd93d
    style A3 fill:#ffd93d
```

**Key Principles:**
- Each actor owns its state
- No shared mutable state
- Communication via messages only
- Perfect for parallelization

---

## Deployment Flexibility

### Single Process (In-Process, Fastest)

```mermaid
graph TB
    subgraph "Single Process"
        subgraph "Shared Arc&lt;MessageBus&gt;"
            MB[MessageBus Core]
        end

        subgraph "Rust Services"
            R1[Market Data<br/>~100ns]
            R2[Strategy<br/>~100ns]
            R3[Execution<br/>~100ns]
        end

        subgraph "Python Services (FFI)"
            P1[ML Model<br/>~500ns]
            P2[Analytics<br/>~500ns]
        end
    end

    R1 --> MB
    R2 --> MB
    R3 --> MB
    P1 --> MB
    P2 --> MB

    style MB fill:#4a9eff
    style R1 fill:#50c878
    style R2 fill:#50c878
    style R3 fill:#50c878
    style P1 fill:#ffd93d
    style P2 fill:#ffd93d
```

**Performance:**
- Rust ↔ Rust: ~100ns (Arc<T>)
- Python ↔ Rust: ~100-500ns (FFI)
- All in shared memory

### Multi-Process (Isolated, Shared Memory)

```mermaid
graph LR
    subgraph "Process 1 (Rust)"
        MB1[MessageBus]
        R1[Market Data]
        R2[Execution]
        R1 --> MB1
        R2 --> MB1
    end

    subgraph "Process 2 (Python)"
        MB2[MessageBus]
        P1[ML Strategy]
        P1 --> MB2
    end

    subgraph "Process 3 (Rust)"
        MB3[MessageBus]
        R3[Risk Engine]
        R3 --> MB3
    end

    MB1 <-->|Shared Memory<br/>~200-500ns| MB2
    MB2 <-->|Shared Memory<br/>~200-500ns| MB3
    MB1 <-->|Shared Memory<br/>~200-500ns| MB3

    style MB1 fill:#4a9eff
    style MB2 fill:#4a9eff
    style MB3 fill:#4a9eff
```

**Benefits:**
- Process isolation (crashes don't propagate)
- Python GIL doesn't block Rust
- ~200-500ns latency (10-50x faster than sockets)

### Distributed (Multi-Machine)

```mermaid
graph TB
    subgraph "Machine 1 (NYC)"
        M1[MessageBus<br/>Market Data]
    end

    subgraph "Machine 2 (London)"
        M2[MessageBus<br/>Strategy Engine]
    end

    subgraph "Machine 3 (Tokyo)"
        M3[MessageBus<br/>Execution]
    end

    M1 <-->|TCP<br/>~1-10ms| M2
    M2 <-->|TCP<br/>~10-50ms| M3
    M1 <-->|TCP<br/>~50-150ms| M3

    style M1 fill:#ff6b6b
    style M2 fill:#ff6b6b
    style M3 fill:#ff6b6b
```

**Use Cases:**
- Geographic distribution
- Kubernetes/cloud deployments
- Regulatory requirements (data residency)

---

## Transport Layer

### Transport Selection Decision Tree

```mermaid
graph TD
    Start[Message Send] --> SameProcess{Same Process?}

    SameProcess -->|Yes| RustOnly{Pure Rust?}
    SameProcess -->|No| SameMachine{Same Machine?}

    RustOnly -->|Yes| ArcT[Arc&lt;T&gt;<br/>~100ns]
    RustOnly -->|No| FFI[FFI<br/>~100-500ns]

    SameMachine -->|Yes| NeedIsolation{Need Isolation?}
    SameMachine -->|No| TCP[TCP Socket<br/>~1-10ms]

    NeedIsolation -->|Yes, Fast| Shm[Shared Memory<br/>~200-500ns]
    NeedIsolation -->|Yes, Simple| Unix[Unix Socket<br/>~1-10μs]
    NeedIsolation -->|No| FFI

    style ArcT fill:#50c878
    style FFI fill:#ffd93d
    style Shm fill:#4a9eff
    style Unix fill:#ff9f43
    style TCP fill:#ff6b6b
```

### Transport Implementation Stack

```mermaid
graph TB
    subgraph "Application Layer"
        Pub[Publisher&lt;T&gt;]
        Sub[Subscriber&lt;T&gt;]
    end

    subgraph "Protocol Layer"
        TLV[TLV Encoding<br/>Type-Length-Value]
        Digest[Schema Digest<br/>Validation]
    end

    subgraph "Transport Layer"
        Local[Local<br/>Arc channels]
        Socket[Socket<br/>Unix/TCP]
        Shm[Shared Memory<br/>Ring Buffer]
    end

    subgraph "OS Layer"
        Mem[Memory]
        Syscall[Syscalls]
        Network[Network]
    end

    Pub --> TLV
    Sub --> TLV
    TLV --> Digest
    Digest --> Local
    Digest --> Socket
    Digest --> Shm

    Local --> Mem
    Socket --> Syscall
    Shm --> Mem
    Socket --> Network

    style TLV fill:#50c878
    style Local fill:#4a9eff
    style Shm fill:#4a9eff
    style Socket fill:#ff9f43
```

---

## Message Flow

### Local Transport (In-Process)

```mermaid
sequenceDiagram
    participant P as Publisher
    participant MB as MessageBus
    participant C as Channel
    participant S as Subscriber

    P->>MB: publish(msg)
    MB->>MB: Arc::new(msg)
    MB->>C: send(Arc&lt;msg&gt;)
    C->>S: recv() → Arc&lt;msg&gt;

    Note over P,S: Zero-copy: ~100ns
```

### FFI Transport (Cross-Language, In-Process)

```mermaid
sequenceDiagram
    participant Py as Python Service
    participant FFI as FFI Layer
    participant MB as MessageBus (Rust)
    participant S as Subscriber (Rust)

    Py->>FFI: publish(msg)
    FFI->>FFI: Serialize to TLV
    FFI->>MB: publish_bytes()
    MB->>MB: Arc::new(bytes)
    MB->>S: recv() → Arc&lt;bytes&gt;
    S->>S: Deserialize from TLV

    Note over Py,S: Shared Arc: ~100-500ns
```

### Shared Memory Transport (Cross-Process)

```mermaid
sequenceDiagram
    participant P1 as Process 1
    participant W as ShmWriter
    participant Ring as Ring Buffer<br/>(mmap)
    participant R as ShmReader
    participant P2 as Process 2

    P1->>W: write_frame(type_id, payload)
    W->>Ring: Atomic write_index update
    Ring->>Ring: Store frame

    loop Poll
        P2->>R: read_frame()
        R->>Ring: Atomic read_index check
    end

    Ring->>R: Frame available
    R->>P2: return (type_id, payload)

    Note over P1,P2: Lock-free: ~200-500ns
```

### Socket Transport (Cross-Process/Machine)

```mermaid
sequenceDiagram
    participant S1 as Service A
    participant Sock1 as Socket Writer
    participant OS as OS Network Stack
    participant Sock2 as Socket Reader
    participant S2 as Service B

    S1->>Sock1: send(frame)
    Sock1->>OS: write() syscall
    OS->>OS: TCP/IP processing
    OS->>Sock2: recv() syscall
    Sock2->>S2: deliver(frame)

    Note over S1,S2: Syscalls: ~1-10μs (Unix)<br/>~1-10ms (TCP)
```

---

## Parallel Backtesting

### Sequential Backtesting (Traditional Approach)

```mermaid
graph LR
    subgraph "Sequential Execution"
        P1[Params 1] --> BT1[Backtest]
        BT1 --> R1[Result]
        R1 --> P2[Params 2]
        P2 --> BT2[Backtest]
        BT2 --> R2[Result]
        R2 --> P3[Params 3]
        P3 --> BT3[Backtest]
        BT3 --> R3[Result]
    end

    R3 --> Analysis[Analyze Results]

    style BT1 fill:#ff6b6b
    style BT2 fill:#ff6b6b
    style BT3 fill:#ff6b6b
```

**Time**: N × backtest_duration

### Mycelium Parallel Backtesting (Actor-Per-Portfolio)

```mermaid
graph TB
    Params[Parameter Grid<br/>10,000 combinations]

    subgraph "Parallel Execution (128 cores)"
        subgraph "Instance 1"
            MB1[MessageBus]
            S1[Strategy]
            P1[Portfolio]
            E1[Exchange Sim]
            S1 --> MB1
            P1 --> MB1
            E1 --> MB1
        end

        subgraph "Instance 2"
            MB2[MessageBus]
            S2[Strategy]
            P2[Portfolio]
            E2[Exchange Sim]
            S2 --> MB2
            P2 --> MB2
            E2 --> MB2
        end

        subgraph "..."
            Dots[...]
        end

        subgraph "Instance 10,000"
            MB3[MessageBus]
            S3[Strategy]
            P3[Portfolio]
            E3[Exchange Sim]
            S3 --> MB3
            P3 --> MB3
            E3 --> MB3
        end
    end

    Params --> MB1
    Params --> MB2
    Params --> Dots
    Params --> MB3

    MB1 --> Results[10,000 Results]
    MB2 --> Results
    Dots --> Results
    MB3 --> Results

    Results --> Best[Find Best Parameters]

    style MB1 fill:#50c878
    style MB2 fill:#50c878
    style MB3 fill:#50c878
```

**Time**: backtest_duration (parallelized across all cores)

### Monte Carlo Simulation

```mermaid
graph TB
    Market[Market Scenarios<br/>10,000 paths]

    subgraph "Parallel Simulation"
        Sim1[Simulation 1<br/>MessageBus + Portfolio]
        Sim2[Simulation 2<br/>MessageBus + Portfolio]
        Sim3[...]
        Sim4[Simulation 10,000<br/>MessageBus + Portfolio]
    end

    Market --> Sim1
    Market --> Sim2
    Market --> Sim3
    Market --> Sim4

    Sim1 --> Dist[Risk Distribution]
    Sim2 --> Dist
    Sim3 --> Dist
    Sim4 --> Dist

    Dist --> VaR[Value at Risk<br/>95% Confidence]

    style Sim1 fill:#4a9eff
    style Sim2 fill:#4a9eff
    style Sim4 fill:#4a9eff
    style VaR fill:#ffd93d
```

**Key Advantage**: Each simulation is completely isolated (no state contamination)

---

## Performance Spectrum

### Latency Comparison

```mermaid
graph LR
    subgraph "Performance Spectrum"
        Arc["Arc&lt;T&gt;<br/>(Rust only)<br/>~100ns"]
        FFI["FFI<br/>(Python in-process)<br/>~100-500ns"]
        Shm["Shared Memory<br/>(Process isolated)<br/>~200-500ns"]
        Unix["Unix Socket<br/>(Process isolated)<br/>~1-10μs"]
        TCP["TCP Socket<br/>(Distributed)<br/>~1-10ms"]
    end

    Arc -->|2-5x slower| FFI
    FFI -->|2-4x slower| Shm
    Shm -->|5-20x slower| Unix
    Unix -->|100-1000x slower| TCP

    style Arc fill:#50c878
    style FFI fill:#a8e6cf
    style Shm fill:#ffd93d
    style Unix fill:#ffb347
    style TCP fill:#ff6b6b
```

### Performance vs Isolation Tradeoff

```mermaid
graph TD
    Start[Choose Transport]

    Start --> Speed{Speed Priority?}

    Speed -->|Absolute Fastest| Rust[Pure Rust<br/>Arc&lt;T&gt;<br/>~100ns]
    Speed -->|Need Python| Safety{Safety Priority?}

    Safety -->|Speed > Safety| FFI_Path[FFI In-Process<br/>~100-500ns<br/>⚠️ Shared process]
    Safety -->|Safety > Speed| Isolation{How much isolation?}

    Isolation -->|Process isolation<br/>+ Speed| Shm_Path[Shared Memory<br/>~200-500ns<br/>✅ Isolated]
    Isolation -->|Process isolation<br/>+ Simple| Unix_Path[Unix Socket<br/>~1-10μs<br/>✅ Isolated]
    Isolation -->|Network isolation| TCP_Path[TCP Socket<br/>~1-10ms<br/>✅ Fully isolated]

    style Rust fill:#50c878
    style FFI_Path fill:#ffd93d
    style Shm_Path fill:#4a9eff
    style Unix_Path fill:#ff9f43
    style TCP_Path fill:#ff6b6b
```

---

## Cross-Language Integration

### Python FFI (In-Process)

```mermaid
graph TB
    subgraph "Single Process Memory Space"
        subgraph "Rust Side"
            MB_Rust[Arc&lt;MessageBus&gt;]
            R1[Rust Service A]
            R2[Rust Service B]
        end

        subgraph "FFI Boundary"
            FFI[PyO3 FFI Layer<br/>~200-300ns overhead]
        end

        subgraph "Python Side"
            MB_Py[Runtime<br/>(Arc ref via FFI)]
            P1[Python Service]
            P2[ML Model]
        end
    end

    R1 --> MB_Rust
    R2 --> MB_Rust
    MB_Rust <-->|Direct Arc access| FFI
    FFI <-->|Python bindings| MB_Py
    P1 --> MB_Py
    P2 --> MB_Py

    style MB_Rust fill:#4a9eff
    style FFI fill:#ffd93d
    style MB_Py fill:#ffd93d
```

**Overhead Breakdown:**
- TLV serialization: ~200-300ns (unavoidable for cross-language)
- FFI call overhead: ~100-200ns
- Total: ~100-500ns

### Python Socket (Out-of-Process)

```mermaid
graph LR
    subgraph "Process 1 (Rust)"
        MB1[MessageBus]
        R1[Rust Services]
        Sock1[Socket Endpoint]
        R1 --> MB1
        MB1 --> Sock1
    end

    subgraph "Process 2 (Python)"
        Sock2[Socket Transport]
        MB2[MessageBus]
        P1[Python Services]
        Sock2 --> MB2
        MB2 --> P1
    end

    Sock1 <-->|Unix/TCP<br/>~1-10μs| Sock2

    style MB1 fill:#4a9eff
    style MB2 fill:#ffd93d
```

**Benefits:**
- Process isolation (crash safety)
- No FFI complexity
- Traditional deployment model

### Hybrid Deployment

```mermaid
graph TB
    subgraph "Process 1: Core Engine (Rust)"
        MB1[MessageBus]
        MD[Market Data]
        Exec[Execution]
        MD --> MB1
        Exec --> MB1
    end

    subgraph "Process 2: Fast ML (Python FFI)"
        MB2[MessageBus + Runtime]
        ML1[Fast Signal Generator<br/>FFI: ~500ns]
        ML1 --> MB2
    end

    subgraph "Process 3: Heavy ML (Python Isolated)"
        MB3[MessageBus]
        ML2[Deep Learning Model<br/>Shm: ~500ns]
        ML2 --> MB3
    end

    MB1 <-->|Shared Memory<br/>~200ns| MB2
    MB1 <-->|Shared Memory<br/>~500ns| MB3
    MB2 <-->|Shared Memory<br/>~500ns| MB3

    style MB1 fill:#50c878
    style MB2 fill:#ffd93d
    style MB3 fill:#ff9f43
```

**Strategy:**
- Critical path in Rust: ~100ns
- Fast Python signals via FFI: ~500ns
- Heavy Python models isolated: ~500ns with safety

---

## Topology-Driven Deployment

### Configuration-Based Routing

```mermaid
graph TB
    subgraph "topology.toml"
        Config["
        [[nodes]]
        name = 'engine'
        services = ['market-data', 'execution']

        [[nodes]]
        name = 'ml-node'
        services = ['strategy']
        endpoint.kind = 'shm'
        endpoint.path = '/dev/shm/ml.shm'
        "]
    end

    subgraph "Runtime Auto-Selection"
        Parser[Topology Parser]
        Router[Smart Router]
    end

    subgraph "Deployment"
        subgraph "Process 1"
            MD[market-data]
            EX[execution]
        end

        subgraph "Process 2"
            ST[strategy]
        end
    end

    Config --> Parser
    Parser --> Router
    Router -->|In-Process<br/>Arc&lt;T&gt;| MD
    Router -->|In-Process<br/>Arc&lt;T&gt;| EX
    Router -->|Shared Memory<br/>~200ns| ST
    MD <-->|Same Bus| EX
    MD <-->|Shm Bridge| ST

    style Config fill:#a8e6cf
    style Router fill:#ffd93d
```

**Developer Experience:**
```rust
// Same code, different deployments via topology.toml
let bus = MessageBus::from_topology("topology.toml", "my-service")?;
let pub = bus.publisher::<Signal>();  // Auto-routes!
```

---

## Summary

### Key Architectural Principles

1. **Actor Model**: Each service is an independent actor with isolated state
2. **Message-Driven**: All communication via typed messages (no shared mutable state)
3. **Transport Agnostic**: Same API works across Arc/FFI/Shm/Socket
4. **Deployment Flexible**: Choose performance/isolation tradeoff without code changes
5. **Parallel-First**: Designed for embarrassingly parallel workloads

### Performance Hierarchy

| Transport | Latency | Isolation | Use Case |
|-----------|---------|-----------|----------|
| Arc<T> | ~100ns | None | Pure Rust, maximum speed |
| FFI | ~100-500ns | None | Python in-process, fast |
| Shared Memory | ~200-500ns | ✅ Process | Python isolated, very fast |
| Unix Socket | ~1-10μs | ✅ Process | Simple, good enough |
| TCP Socket | ~1-10ms | ✅ Network | Distributed, geo-dispersed |

### Killer Features

1. **Same-code, multi-deployment**: Write once, deploy anywhere (in-process → distributed)
2. **Parallel backtesting**: Spawn 10,000 independent actor systems trivially
3. **Language flexibility**: Rust, Python, OCaml with minimal overhead
4. **Compile-time optimization**: Rust monomorphization eliminates runtime branching

---

**See Also:**
- [Cross-Language Integration](CROSS_LANGUAGE_INTEGRATION.md)
- [Deployment Options](../DEPLOYMENT_OPTIONS.md)
- [Shared Memory Transport](../implementation/SHARED_MEMORY_TRANSPORT.md)
