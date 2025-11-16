# Shared Memory Transport Implementation Plan

## Goal

Implement a **shared memory transport** that allows Python/OCaml services to achieve near-Arc<T> performance (~100-500ns latency) while maintaining process isolation and language-agnostic integration.

## Why This Matters

- **Closes the performance gap**: Python gets ~10-100x better latency vs Unix sockets
- **Maintains safety**: Still process-isolated (unlike FFI)
- **"No downsides" design**: Achieves parity with Rust's in-process performance
- **Language-agnostic**: Works with any language that supports mmap

## Architecture

### Ring Buffer Design

Use a **lock-free SPSC (Single Producer, Single Consumer) ring buffer** in shared memory:

```
/dev/shm/mycelium/<service_name>.shm
┌────────────────────────────────────────────────┐
│ Header (64 bytes, cache-line aligned)         │
│  - write_index: AtomicU64                     │
│  - read_index: AtomicU64                      │
│  - capacity: u64                              │
│  - schema_digest: [u8; 32]                    │
│  - version: u32                               │
│  - flags: u32                                 │
└────────────────────────────────────────────────┘
┌────────────────────────────────────────────────┐
│ Ring Buffer (configurable size, e.g., 1MB)    │
│  [Frame 0: len(u32) + type_id(u16) + pad(u16) │
│            + payload(len bytes)]              │
│  [Frame 1: ...]                               │
│  ...                                          │
└────────────────────────────────────────────────┘
```

### Frame Format

Each frame in the ring buffer:

```
┌──────────┬─────────┬─────────┬─────────────────┐
│ Length   │ Type ID │ Padding │ Payload         │
│ (u32)    │ (u16)   │ (u16)   │ (N bytes)       │
└──────────┴─────────┴─────────┴─────────────────┘
 4 bytes    2 bytes   2 bytes   variable
```

- **Length**: Total frame size (header + payload)
- **Type ID**: Message type identifier
- **Padding**: Reserved for alignment
- **Payload**: TLV-encoded message

### Memory Layout

```rust
#[repr(C)]
struct ShmHeader {
    write_index: AtomicU64,       // Writer's current position
    read_index: AtomicU64,        // Reader's current position
    capacity: u64,                // Ring buffer size in bytes
    schema_digest: [u8; 32],      // Schema version check
    version: u32,                 // Protocol version
    flags: u32,                   // Reserved for future use
}

const HEADER_SIZE: usize = 64;
const DEFAULT_BUFFER_SIZE: usize = 1024 * 1024; // 1MB
```

## Implementation Phases

### Phase 1: Rust Side (Core)

1. **Create `SharedMemoryRingBuffer` crate**:
   - `ShmHeader` struct with atomic indices
   - `RingBufferWriter` with `write_frame()` method
   - `RingBufferReader` with `read_frame()` method
   - Memory ordering: `Acquire`/`Release` for cross-process sync

2. **Add `SharedMemoryTransport` to `mycelium-transport`**:
   - `bind_shm_endpoint()` - creates shared memory region
   - `connect_shm_endpoint()` - connects to existing region
   - Integrates with `BridgeFanout` for outgoing frames
   - Spawns reader task for incoming frames

3. **API Surface**:
   ```rust
   // Rust API
   pub async fn bind_shm_endpoint(
       path: &str,
       local: LocalTransport,
       schema_digest: [u8; 32],
       capacity: Option<usize>,
   ) -> Result<ShmEndpointHandle>
   ```

### Phase 2: Python Side

1. **Create `SharedMemoryTransport` in Python SDK**:
   - Use `mmap` module for memory mapping
   - Use `ctypes` for atomic operations (or fall back to lock)
   - Implement same ring buffer protocol

2. **API Surface**:
   ```python
   from mycelium import SharedMemoryTransport

   transport = SharedMemoryTransport(
       "/dev/shm/mycelium/service.shm",
       schema_digest=SCHEMA_DIGEST
   )
   transport.connect()
   pub = transport.publisher(Message)
   sub = transport.subscriber(Message)
   ```

### Phase 3: Integration with MessageBus

Update `MessageBus.from_topology()` to support shared memory:

```toml
# topology.toml
[[nodes]]
name = "python-worker"
services = ["ml-model"]
endpoint.kind = "shm"  # <-- New endpoint type
endpoint.path = "/dev/shm/mycelium/python-worker.shm"
```

```python
bus = MessageBus.from_topology("topology.toml", "ml-model", DIGEST)
# Auto-selects SharedMemoryTransport if endpoint.kind = "shm"
```

## Synchronization Strategy

### Write Path (Producer)

```rust
fn write_frame(&mut self, frame: &[u8]) -> Result<()> {
    let frame_size = frame.len();
    let write_idx = self.header.write_index.load(Ordering::Acquire);
    let read_idx = self.header.read_index.load(Ordering::Acquire);

    // Check if buffer has space
    let available = self.capacity - (write_idx - read_idx);
    if available < frame_size {
        return Err(BufferFull);
    }

    // Write frame (may wrap around)
    let pos = (write_idx % self.capacity) as usize;
    self.write_wrapping(pos, frame);

    // Publish write with Release ordering
    self.header.write_index.store(
        write_idx + frame_size as u64,
        Ordering::Release
    );

    Ok(())
}
```

### Read Path (Consumer)

```rust
fn read_frame(&mut self) -> Option<Vec<u8>> {
    let write_idx = self.header.write_index.load(Ordering::Acquire);
    let read_idx = self.header.read_index.load(Ordering::Acquire);

    // Check if data available
    if write_idx == read_idx {
        return None; // Empty
    }

    // Read frame length
    let pos = (read_idx % self.capacity) as usize;
    let frame_len = self.read_u32(pos);

    // Read full frame
    let frame = self.read_wrapping(pos, frame_len as usize);

    // Publish read with Release ordering
    self.header.read_index.store(
        read_idx + frame_len as u64,
        Ordering::Release
    );

    Some(frame)
}
```

## Performance Expectations

| Operation | Latency | Notes |
|-----------|---------|-------|
| **Write (Rust)** | ~50-100ns | Atomic store + memcpy |
| **Read (Rust)** | ~50-100ns | Atomic load + memcpy |
| **Round-trip (Rust ↔ Python)** | ~200-500ns | Write + Read + context switch |
| **vs Unix Socket** | **10-50x faster** | Unix socket: ~1-10μs |
| **vs Arc<T>** | **2-5x slower** | Arc<T>: ~100ns (no serialization) |

## Failure Modes & Recovery

### Buffer Full

**Problem**: Writer fills buffer faster than reader consumes

**Solution**: Block writer or drop frames (configurable)

```rust
pub enum Backpressure {
    Block,        // Wait for space (default)
    DropOldest,   // Overwrite oldest frame
    DropNewest,   // Drop new frame
}
```

### Reader Crash

**Problem**: Reader crashes, write index keeps advancing

**Solution**: Detect stale readers via heartbeat or cleanup on reconnect

```rust
// Check if reader is stale (hasn't read in 5 seconds)
let last_read_time = self.header.last_read_timestamp.load(Ordering::Acquire);
if now - last_read_time > Duration::from_secs(5) {
    self.reset_reader_index();
}
```

### Writer Crash

**Problem**: Writer crashes mid-write, partial frame in buffer

**Solution**: Use sequence numbers to detect corruption

```rust
struct FrameHeader {
    length: u32,
    type_id: u16,
    sequence: u16,  // Incremented per frame
}
```

## Platform Support

| Platform | Mechanism | Path Template |
|----------|-----------|---------------|
| **Linux** | `/dev/shm` (tmpfs) | `/dev/shm/mycelium/<name>.shm` |
| **macOS** | `shm_open()` + `mmap()` | `/tmp/mycelium/<name>.shm` |
| **Windows** | `CreateFileMapping()` | `Global\\mycelium_<name>` |

Use `memmap2` crate for cross-platform abstraction.

## Security Considerations

1. **Permissions**: Shared memory files should be `0600` (owner-only)
2. **Cleanup**: Remove shm files on graceful shutdown
3. **Schema validation**: Always check digest before reading frames
4. **Buffer overflow**: Bounds check all reads/writes

## Testing Strategy

### Unit Tests

- Ring buffer wraparound correctness
- Concurrent write/read (single producer/consumer)
- Buffer full handling
- Schema digest mismatch detection

### Integration Tests

- Rust → Python frame delivery
- Python → Rust frame delivery
- Crash recovery (kill reader/writer mid-transfer)
- Performance benchmarks (compare vs Unix socket)

### Benchmark Targets

```
SharedMemory_write           50-100ns
SharedMemory_read            50-100ns
SharedMemory_roundtrip       200-500ns
UnixSocket_roundtrip         1-10μs     (baseline)
Arc_roundtrip                ~100ns     (theoretical minimum)
```

## Migration Path

### v0.2.0 (Current)

- Unix/TCP socket transports
- Topology-aware routing

### v0.3.0 (Shared Memory)

- Add `SharedMemoryTransport` to Rust
- Add Python bindings
- Update `MessageBus` to auto-select shared memory
- Benchmark and optimize

### v0.4.0 (Production Hardening)

- Add OCaml shared memory support
- Multi-reader support (MPSC)
- Advanced backpressure strategies
- Monitoring integration

## Open Questions

1. **Buffer size**: Start with 1MB default, make configurable?
2. **Multi-reader**: SPSC first, add MPSC later?
3. **Blocking vs async**: Use `tokio::task::spawn_blocking` for reads?
4. **Alignment**: Force 8-byte or cache-line (64-byte) alignment?

## Implementation Estimate

| Task | Effort | Priority |
|------|--------|----------|
| Rust ring buffer | 4-6 hours | P0 |
| Rust transport integration | 2-3 hours | P0 |
| Python ring buffer | 3-4 hours | P0 |
| Python transport integration | 2-3 hours | P0 |
| Tests | 2-3 hours | P0 |
| Benchmarks | 1-2 hours | P1 |
| Documentation | 1-2 hours | P1 |
| **Total** | **15-23 hours** | - |

## Success Criteria

✅ Python ↔ Rust round-trip latency < 1μs (p50)
✅ Throughput > 1M messages/second (small messages)
✅ No memory corruption under stress testing
✅ Graceful degradation on buffer full
✅ Topology integration works seamlessly

## References

- [Linux `shm_open(3)` man page](https://man7.org/linux/man-pages/man3/shm_open.3.html)
- [Rust `memmap2` crate](https://docs.rs/memmap2)
- [Python `mmap` module](https://docs.python.org/3/library/mmap.html)
- [Atomic memory ordering](https://doc.rust-lang.org/nomicon/atomics.html)

---

**Status**: Ready to implement (priority: P0 for v0.3.0)
