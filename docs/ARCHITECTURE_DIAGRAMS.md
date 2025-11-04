# Mycelium Architecture Diagrams

This document contains ASCII art diagrams illustrating the Mycelium architecture.

---

## 1. System Overview - Three-Layer Architecture

```
┌────────────────────────────────────────────────────────────────────────────┐
│                          Application Layer                                  │
│                                                                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │ Arbitrage    │  │ Risk         │  │ Execution    │  │ Market Data  │  │
│  │ Service      │  │ Manager      │  │ Service      │  │ Adapter      │  │
│  └──────────────┘  └──────────────┘  └──────────────┘  └──────────────┘  │
│         │                 │                  │                  │           │
│         └─────────────────┴──────────────────┴──────────────────┘           │
│                                      │                                       │
└──────────────────────────────────────┼───────────────────────────────────────┘
                                       │
┌──────────────────────────────────────┼───────────────────────────────────────┐
│                          Transport Layer                                     │
│                                      │                                       │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                         MessageBus                                   │   │
│  │  ┌────────────────┐  ┌────────────────┐  ┌────────────────┐       │   │
│  │  │ Publisher<M>    │  │ Subscriber<M>   │  │ ActorRuntime    │       │   │
│  │  └────────────────┘  └────────────────┘  └────────────────┘       │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                      │                                       │
│  ┌───────────────────────────────────┴──────────────────────────────────┐  │
│  │              Transport Selection (Topology-Aware)                     │  │
│  │  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐           │  │
│  │  │ Arc<T>         │  │ Unix Socket    │  │ TCP           │           │  │
│  │  │ (Same Process) │  │ (IPC)          │  │ (Network)     │           │  │
│  │  │ 65ns           │  │ 1μs            │  │ 10μs          │           │  │
│  │  └───────────────┘  └───────────────┘  └───────────────┘           │  │
│  └───────────────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────┼───────────────────────────────────┘
                                           │
┌──────────────────────────────────────────┼───────────────────────────────────┐
│                          Protocol Layer                                      │
│                                          │                                   │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                       TLV Codec & Schema Evolution                   │   │
│  │  ┌────────────────┐  ┌────────────────┐  ┌────────────────┐       │   │
│  │  │ encode_message │  │ decode_message │  │ check_compat   │       │   │
│  │  │ (zerocopy)     │  │ (zerocopy)     │  │ (schema)       │       │   │
│  │  └────────────────┘  └────────────────┘  └────────────────┘       │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                          │                                   │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                       Message Definitions                            │   │
│  │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐    │   │
│  │  │ PoolStateUpdate │  │ ArbitrageSignal │  │ InstrumentMeta  │    │   │
│  │  │ TYPE_ID: 1011   │  │ TYPE_ID: 1020   │  │ TYPE_ID: 1018   │    │   │
│  │  └─────────────────┘  └─────────────────┘  └─────────────────┘    │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────────────────────────┘
```

---

## 2. TLV Wire Format

```
Message on Wire:
┌──────────┬──────────┬──────────────┬──────────────┬─────────────────────────────┐
│ Type ID  │ Length   │ Schema Ver   │ (Reserved)   │ Payload (zerocopy)          │
│ 2 bytes  │ 4 bytes  │ 2 bytes      │ 0 bytes      │ N bytes                     │
│ u16 LE   │ u32 LE   │ u16 LE       │              │ (binary, #[repr(C)] layout) │
└──────────┴──────────┴──────────────┴──────────────┴─────────────────────────────┘
   1011       56          1             (pad)          [pool_address: [u8;20], ...]
 (PoolState)  (bytes)   (v1)                           

Total Header: 8 bytes
Max Payload: 4GB (u32::MAX)

Deserialization:
1. Parse header (validate TYPE_ID)     : 2ns
2. Pointer cast to struct (zerocopy)   : 0ns
   ----------------------------------------
   Total: 2ns (vs 100-500ns for Protobuf)
```

---

## 3. Message Flow: Publish/Subscribe

```
Publisher Side:
┌─────────────────────────────────────────────────────────────────────────┐
│ Application                                                              │
│   pub_.publish(PoolStateUpdate { ... }).await?                         │
│     │                                                                    │
│     ├──> 1. Wrap in Envelope (Arc allocation)                          │
│     │      Envelope { type_id: 1011, payload: Arc<PoolStateUpdate> }   │
│     │                                                                    │
│     ├──> 2. Send to broadcast channel                                  │
│     │      tx.send(envelope)  // tokio::sync::broadcast               │
│     │                                                                    │
│     └──> 3. Return receiver count                                      │
│            if count == 0 { warn!("No subscribers") }                    │
└─────────────────────────────────────────────────────────────────────────┘

Subscriber Side:
┌─────────────────────────────────────────────────────────────────────────┐
│ Application                                                              │
│   let msg = sub.recv().await?                                          │
│     │                                                                    │
│     ├──> 1. Receive from channel                                       │
│     │      envelope = rx.recv().await  // Arc clone (cheap)           │
│     │                                                                    │
│     ├──> 2. Downcast payload                                           │
│     │      envelope.downcast_ref::<PoolStateUpdate>()                  │
│     │                                                                    │
│     └──> 3. Return message reference                                   │
│            Arc::try_unwrap() or Arc::clone()                            │
└─────────────────────────────────────────────────────────────────────────┘

Timeline:
                    Publisher                    Subscriber
                       │                             │
t=0ns                  │ publish()                   │
t=10ns                 │ ─ Envelope wrap ─>          │
t=50ns                 │ ─ Arc alloc ────>           │
t=60ns                 │ ─ channel.send ─>           │
t=65ns                 │                             │ recv()
t=70ns                 │                             │ Arc clone
t=75ns                 │                             │ downcast
t=80ns                 │                             │ <msg available>
                       │                             │

Total latency: 65ns (Arc transport)
```

---

## 4. Actor System: Mailbox-Based Communication

```
Actor Spawning:
┌─────────────────────────────────────────────────────────────────────────┐
│ ActorRuntime::spawn(MyActor::new()).await                               │
│   │                                                                      │
│   ├──> 1. Generate unique ActorId                                      │
│   │      actor_id = ActorId::new()  // UUID                            │
│   │                                                                      │
│   ├──> 2. Create mailbox topic                                         │
│   │      topic = format!("actor.{:016x}", actor_id)                    │
│   │                                                                      │
│   ├──> 3. Subscribe to mailbox                                         │
│   │      subscriber = bus.subscriber_for_topic(&topic)                 │
│   │                                                                      │
│   ├──> 4. Spawn actor task                                             │
│   │      tokio::spawn(async move {                                     │
│   │          loop {                                                     │
│   │              msg = subscriber.recv().await                          │
│   │              actor.handle(msg, &mut ctx).await                      │
│   │          }                                                           │
│   │      })                                                              │
│   │                                                                      │
│   ├──> 5. Register for typed discovery                                 │
│   │      type_registry.insert(TypeId::of::<MyActor>(), actor_id)       │
│   │                                                                      │
│   └──> 6. Return ActorRef                                              │
│          ActorRef { actor_id, publisher }                               │
└─────────────────────────────────────────────────────────────────────────┘

Message Sending:
┌─────────────────────────────────────────────────────────────────────────┐
│ actor_ref.send(MyMessage { ... }).await?                                │
│   │                                                                      │
│   ├──> 1. Get publisher for actor's mailbox                            │
│   │      pub_ = bus.publisher_for_topic("actor.{actor_id}")            │
│   │                                                                      │
│   └──> 2. Publish to actor's mailbox                                   │
│          pub_.publish(msg).await  // Standard pub/sub                   │
└─────────────────────────────────────────────────────────────────────────┘

Actor Discovery:
┌─────────────────────────────────────────────────────────────────────────┐
│ runtime.get_actor::<RiskManager>().await?                               │
│   │                                                                      │
│   ├──> 1. Get TypeId                                                    │
│   │      type_id = TypeId::of::<RiskManager>()                          │
│   │                                                                      │
│   ├──> 2. Lookup ActorId                                                │
│   │      actor_id = type_registry.get(&type_id)?                        │
│   │                                                                      │
│   └──> 3. Return ActorRef                                               │
│          ActorRef { actor_id, publisher }                                │
└─────────────────────────────────────────────────────────────────────────┘

Actor Hierarchy:
                      ┌───────────────────┐
                      │   ActorRuntime     │
                      │   (Supervisor)     │
                      └───────────────────┘
                              │
                ┌─────────────┼─────────────┐
                │             │             │
         ┌──────▼──────┐ ┌───▼────────┐ ┌─▼────────────┐
         │ RiskManager │ │ Arbitrage  │ │ Execution    │
         │ Actor       │ │ Actor      │ │ Actor        │
         └─────────────┘ └────────────┘ └──────────────┘
                │
         ┌──────┴──────┐
         │  Mailbox    │
         │  (channel)  │
         └─────────────┘
```

---

## 5. Transport Selection Decision Tree

```
Application calls: bus.publisher_to("target-service")?

                    ┌──────────────────────────────┐
                    │  Topology Configuration      │
                    │  (TOML or runtime map)       │
                    └──────────────┬───────────────┘
                                   │
                    ┌──────────────▼───────────────┐
                    │ Is "target-service" on       │
                    │ same node as caller?         │
                    └──────────────┬───────────────┘
                                   │
                    ┌──────────────┴───────────────┐
                    │                              │
                  YES                             NO
                    │                              │
         ┌──────────▼──────────┐    ┌─────────────▼────────────┐
         │ Return Arc<T>        │    │ Is "target-service" on   │
         │ Publisher            │    │ same machine?            │
         │ (LocalTransport)     │    └─────────────┬────────────┘
         │                      │                   │
         │ • 65ns latency       │    ┌──────────────┴───────────────┐
         │ • Zero serialization │    │                              │
         │ • broadcast::channel │  YES                             NO
         └──────────────────────┘    │                              │
                                     │                              │
                      ┌──────────────▼──────────┐    ┌─────────────▼────────────┐
                      │ Return Unix Socket       │    │ Return TCP Publisher     │
                      │ Publisher                │    │ (TcpTransport)           │
                      │ (UnixTransport)          │    │                          │
                      │                          │    │ • 10-100μs latency       │
                      │ • 1μs latency            │    │ • TLV serialization      │
                      │ • TLV serialization      │    │ • Network stack          │
                      │ • Kernel context switch  │    │ • TCP connection pooling │
                      └──────────────────────────┘    └──────────────────────────┘

Example Topology:
┌────────────────────────────────────────────────────────────────────────────┐
│ Node 1: "adapters" (10.0.1.10)                                             │
│   Services: [PolygonAdapter, BaseAdapter]                                  │
│   Transport: tcp://10.0.1.10:9000                                          │
└────────────────────────────────────────────────────────────────────────────┘
                                │
                                │ TCP (10μs)
                                │
┌────────────────────────────────────────────────────────────────────────────┐
│ Node 2: "strategies" (10.0.1.11)                                           │
│   Services: [ArbitrageService, RiskManager, ExecutionService]              │
│   Transport: local (Arc<T>)                                                │
│                                                                             │
│   PolygonAdapter ───TCP───> ArbitrageService ───Arc───> RiskManager       │
│                                      │                         │            │
│                                      └───Arc───> ExecutionService           │
└────────────────────────────────────────────────────────────────────────────┘
```

---

## 6. Compile-Time Routing vs Runtime Routing

```
Runtime Routing (MessageBus):
┌────────────────────────────────────────────────────────────────────────────┐
│ Application                                                                 │
│   ctx.emit(V2Swap { ... }).await?                                          │
│     │                                                                        │
│     ├──> 1. Lookup publisher                                               │
│     │      HashMap<String, Publisher<M>>  // Runtime lookup                │
│     │                                                                        │
│     ├──> 2. Wrap in Envelope                                               │
│     │      Envelope { type_id, payload: Arc<V2Swap> }  // 40ns allocation  │
│     │                                                                        │
│     ├──> 3. Send to channel                                                │
│     │      broadcast::send(envelope)  // 10ns                               │
│     │                                                                        │
│     └──> 4. Subscribers receive                                            │
│            for sub in subscribers {                                         │
│                sub.recv().await  // Arc clone per subscriber               │
│            }                                                                 │
│                                                                             │
│   Total: 65ns (flexible, dynamic, supports distributed)                    │
└────────────────────────────────────────────────────────────────────────────┘

Compile-Time Routing:
┌────────────────────────────────────────────────────────────────────────────┐
│ Application                                                                 │
│   system.route_v2_swap(&swap);  // Direct function call                    │
│     │                                                                        │
│     ├──> Generated code (at compile time):                                 │
│     │      #[inline(always)]                                                │
│     │      fn route_v2_swap(&mut self, msg: &V2Swap) {                     │
│     │          self.arbitrage.handle(msg);  // Direct call (2ns)           │
│     │          self.risk.handle(msg);       // Direct call (2ns)           │
│     │      }                                                                 │
│     │                                                                        │
│     └──> Compiler optimizes to:                                            │
│            arbitrage.handle(&swap);  // Potentially inlined (0ns)          │
│            risk.handle(&swap);       // Potentially inlined (0ns)          │
│                                                                             │
│   Total: 2-4ns (fixed, static, single-process only)                        │
└────────────────────────────────────────────────────────────────────────────┘

Code Generation:
routing_config! {                    // Macro invocation
    name: TradingSystem,
    routes: {
        V2Swap => [ArbitrageService, RiskManager],
    }
}

   │
   │ Macro expansion at compile time
   ▼

struct TradingSystem {               // Generated struct
    arbitrage: ArbitrageService,
    risk: RiskManager,
}

impl TradingSystem {                 // Generated impl
    #[inline(always)]
    fn route_v2_swap(&mut self, msg: &V2Swap) {
        self.arbitrage.handle(msg);
        self.risk.handle(msg);
    }
}
```

---

## 7. Schema Evolution: Version Compatibility

```
Schema Version Timeline:
v1 (Initial):                v2 (Added field):           v3 (Changed type):
┌──────────────────┐        ┌──────────────────┐       ┌──────────────────┐
│ PoolStateUpdate  │        │ PoolStateUpdate  │       │ PoolStateUpdate  │
│ ─────────────────│        │ ─────────────────│       │ ─────────────────│
│ pool_address     │        │ pool_address     │       │ pool_address     │
│ reserve0         │        │ reserve0         │       │ venue_id (NEW)   │
│ reserve1         │        │ reserve1         │       │ reserve0 (Opt)   │
│ block_number     │        │ block_number     │       │ reserve1 (Opt)   │
│                  │        │ venue_id (NEW!)  │       │ liquidity (NEW)  │
└──────────────────┘        └──────────────────┘       │ block_number     │
                                                         └──────────────────┘

Compatibility Matrix:
                 │ v1 Decoder │ v2 Decoder │ v3 Decoder │
─────────────────┼────────────┼────────────┼────────────│
v1 Message       │     ✓      │     ✓      │     ✓      │ (All can read)
v2 Message       │     ✗      │     ✓      │     ✓      │ (v1 missing field)
v3 Message       │     ✗      │     ✗      │     ✓      │ (Breaking change)

Wire Format with Schema Version:
┌──────────┬──────────┬───────────────┬─────────────────────────────┐
│ Type ID  │ Length   │ Schema Ver    │ Payload                     │
│ 1011     │ 64       │ 2 ◄───────────┤ [pool, reserves, venue_id]  │
└──────────┴──────────┴───────────────┴─────────────────────────────┘
                           │
                           └──> Decoder checks:
                                 if schema_ver > MY_MAX_VER {
                                     return Err(IncompatibleVersion)
                                 }
                                 if schema_ver < MY_MIN_VER {
                                     return Err(TooOld)
                                 }

Handling Unknown Fields:
v2 Decoder receiving v1 message:
┌────────────────────────────────────────────────────────────────────┐
│ Expected: PoolStateUpdate v2                                        │
│   { pool_address, reserve0, reserve1, block_number, venue_id }     │
│                                                                     │
│ Received: PoolStateUpdate v1                                        │
│   { pool_address, reserve0, reserve1, block_number }               │
│                                                                     │
│ Action: Fill missing field with default                            │
│   venue_id = 0  // or Option::None if optional                     │
└────────────────────────────────────────────────────────────────────┘

v1 Decoder receiving v2 message:
┌────────────────────────────────────────────────────────────────────┐
│ Expected: PoolStateUpdate v1                                        │
│   { pool_address, reserve0, reserve1, block_number }               │
│                                                                     │
│ Received: PoolStateUpdate v2                                        │
│   { pool_address, reserve0, reserve1, block_number, venue_id }     │
│                                                                     │
│ Action: Skip unknown field (forward compatibility)                 │
│   Read first 4 fields, ignore venue_id                             │
│                                                                     │
│ Problem: Fixed-size struct doesn't support this!                   │
│   Solution: Use Option<T> for new fields                           │
└────────────────────────────────────────────────────────────────────┘
```

---

## 8. Buffer Pool: Zero-Allocation I/O

```
Buffer Lifecycle:
┌────────────────────────────────────────────────────────────────────────────┐
│ 1. Acquire Buffer                                                           │
│    let buf = pool.acquire(128);  // Request 128-byte buffer                │
│       │                                                                      │
│       ├──> Check pool for available buffer                                 │
│       │      pools.get_mut(128).pop()                                       │
│       │                                                                      │
│       ├──> If empty, allocate new buffer                                   │
│       │      vec![0u8; 128]  // Only happens once per size class           │
│       │                                                                      │
│       └──> Wrap in PooledBuffer                                            │
│              PooledBuffer { data: Vec<u8>, pool: Arc<Pool>, size: 128 }    │
│                                                                             │
│ 2. Use Buffer                                                               │
│    stream.read_exact(&mut buf[..]).await?;  // Read from network           │
│    let msg = decode_message(&buf[..])?;     // Zero-copy decode            │
│                                                                             │
│ 3. Drop Buffer (automatic return to pool)                                  │
│    drop(buf);  // Or goes out of scope                                     │
│       │                                                                      │
│       └──> PooledBuffer::drop() called                                     │
│              pools.get_mut(128).push(self.data)  // Return to pool         │
└────────────────────────────────────────────────────────────────────────────┘

Pool Structure:
┌────────────────────────────────────────────────────────────────────────────┐
│ BufferPool                                                                  │
│ ┌────────────────────────────────────────────────────────────────────────┐ │
│ │ Size Class 64:   [buf1, buf2, buf3] ◄─ Available                       │ │
│ │ Size Class 128:  [buf4, buf5]       ◄─ Available                       │ │
│ │ Size Class 256:  []                 ◄─ All in use                      │ │
│ │ Size Class 512:  [buf6]             ◄─ Available                       │ │
│ └────────────────────────────────────────────────────────────────────────┘ │
│                                                                             │
│ Stats:                                                                      │
│   total_allocations: 6   (1 per size class, 2 for 64-byte)                │
│   total_acquires: 1000   (Buffer acquisitions)                             │
│   total_returns: 995     (5 still in use)                                  │
│   hit_rate: 99.4%        ((1000-6)/1000)                                   │
└────────────────────────────────────────────────────────────────────────────┘

Performance:
Without Pooling:           With Pooling:
┌──────────────────┐      ┌──────────────────┐
│ Read from network│      │ Read from network│
│   ↓              │      │   ↓              │
│ Allocate buffer  │ 40ns │ Acquire from pool│ 5ns
│   ↓              │      │   ↓              │
│ Decode message   │ 2ns  │ Decode message   │ 2ns
│   ↓              │      │   ↓              │
│ Process message  │      │ Process message  │
│   ↓              │      │   ↓              │
│ Free buffer      │ 20ns │ Return to pool   │ 3ns
└──────────────────┘      └──────────────────┘
Total: 62ns overhead      Total: 10ns overhead
                          Savings: 52ns (84% reduction)
```

---

## 9. Performance Latency Breakdown

```
Message Journey (Arc Transport):

  Sender                          Transport                        Receiver
    │                                │                                │
    │ publish(msg)                   │                                │
    ├───────────────────────────────>│                                │
    │                                │                                │
    │                          [Envelope Wrap]                        │
    │                          • Allocate Arc                         │
    │                          • Store type_id                        │
    │                          • Wrap payload                         │
    │                                │ (40ns)                         │
    │                                │                                │
    │                          [Broadcast Send]                       │
    │                          • Channel.send()                       │
    │                          • Wake subscribers                     │
    │                                │ (10ns)                         │
    │<──────────────Ok(count)────────┤                                │
    │                                │                                │
    │                                │ ────Envelope────>              │
    │                                │                  │ recv()      │
    │                                │                  │             │
    │                                │            [Arc Clone]         │
    │                                │            • Clone Arc ptr     │
    │                                │            • Incr ref count    │
    │                                │                  │ (5ns)       │
    │                                │                  │             │
    │                                │            [Downcast]          │
    │                                │            • Type check        │
    │                                │            • Extract payload   │
    │                                │                  │ (10ns)      │
    │                                │                  │             │
    │                                │                  ▼             │
    │                                │            <Message Ready>     │
    │                                │                                │

Timeline:
t=0ns   : publish() called
t=40ns  : Arc allocated, envelope wrapped
t=50ns  : Channel send, subscribers woken
t=55ns  : recv() returns Arc<Envelope>
t=60ns  : Arc cloned (ref count++)
t=65ns  : Downcast to concrete type
t=70ns  : Message available to application

Total Latency: 65-70ns

Compare to:
- Direct function call: 2ns (32x faster)
- serde (MessagePack): 200ns (3x slower)
- Protobuf: 500ns (7x slower)
- JSON: 2000ns (30x slower)
```

---

## 10. QoS and Backpressure Policies

```
Actor Mailbox with DropPolicy:

DropOldest (Ring Buffer):
┌────────────────────────────────────────────────────────────────────────────┐
│ Mailbox (capacity: 5)                                                       │
│ ┌───┬───┬───┬───┬───┐                                                      │
│ │ 1 │ 2 │ 3 │ 4 │ 5 │ ◄─ Full                                              │
│ └───┴───┴───┴───┴───┘                                                      │
│                                                                             │
│ New message arrives (msg 6):                                                │
│ ┌───┬───┬───┬───┬───┐                                                      │
│ │ 2 │ 3 │ 4 │ 5 │ 6 │ ◄─ Dropped msg 1, added msg 6                        │
│ └───┴───┴───┴───┴───┘                                                      │
│                                                                             │
│ Use case: Real-time market data (latest is most important)                 │
└────────────────────────────────────────────────────────────────────────────┘

DropNewest (Keep History):
┌────────────────────────────────────────────────────────────────────────────┐
│ Mailbox (capacity: 5)                                                       │
│ ┌───┬───┬───┬───┬───┐                                                      │
│ │ 1 │ 2 │ 3 │ 4 │ 5 │ ◄─ Full                                              │
│ └───┴───┴───┴───┴───┘                                                      │
│                                                                             │
│ New message arrives (msg 6):                                                │
│ ┌───┬───┬───┬───┬───┐                                                      │
│ │ 1 │ 2 │ 3 │ 4 │ 5 │ ◄─ Dropped msg 6, kept history                       │
│ └───┴───┴───┴───┴───┘                                                      │
│                                                                             │
│ Use case: Batch processing where older data has value                      │
└────────────────────────────────────────────────────────────────────────────┘

Reject (Backpressure):
┌────────────────────────────────────────────────────────────────────────────┐
│ Mailbox (capacity: 5)                                                       │
│ ┌───┬───┬───┬───┬───┐                                                      │
│ │ 1 │ 2 │ 3 │ 4 │ 5 │ ◄─ Full                                              │
│ └───┴───┴───┴───┴───┘                                                      │
│                                                                             │
│ New message arrives (msg 6):                                                │
│    Err(MailboxFull) returned to sender                                     │
│                                                                             │
│ Use case: Critical messages that must not be lost                          │
└────────────────────────────────────────────────────────────────────────────┘

Block (Flow Control):
┌────────────────────────────────────────────────────────────────────────────┐
│ Sender                  Mailbox                       Receiver              │
│   │                       │                             │                   │
│   │ send(msg6)            │                             │                   │
│   ├──────────────────────>│ Full! Block sender          │                   │
│   │                       │ ┌───┬───┬───┬───┬───┐     │                   │
│   │ (blocked...)          │ │ 1 │ 2 │ 3 │ 4 │ 5 │     │                   │
│   │                       │ └───┴───┴───┴───┴───┘     │                   │
│   │                       │                             │ recv(msg1)        │
│   │                       │<─────────────────────────────┤                   │
│   │                       │ Space available!            │                   │
│   ├──────────────────────>│ Accept msg6                 │                   │
│   │ (unblocked)           │ ┌───┬───┬───┬───┬───┐     │                   │
│   │                       │ │ 2 │ 3 │ 4 │ 5 │ 6 │     │                   │
│   │                       │ └───┴───┴───┴───┴───┘     │                   │
│                                                                             │
│ Use case: Default safe behavior (prevent overload)                         │
└────────────────────────────────────────────────────────────────────────────┘

Metrics:
┌────────────────────────────────────────────────────────────────────────────┐
│ MailboxMetrics                                                              │
│ ───────────────────────────────────────────────────────────────────────────│
│ dropped_messages:    142  (cumulative)                                     │
│ queue_depth:          87  (current)                                        │
│ max_latency_us:     1520  (P99.9)                                          │
│ total_processed:   10000  (cumulative)                                     │
│ total_rejected:        5  (when using Reject policy)                       │
│ drop_rate:         1.42%  (142/10000)                                      │
└────────────────────────────────────────────────────────────────────────────┘
```

---

End of diagrams.
