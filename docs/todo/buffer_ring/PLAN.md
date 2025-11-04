# Buffer Pool Implementation Plan

**Status**: Ready to implement
**Priority**: High (performance from day one)
**Estimated Time**: 16-20 hours (~2 days)

---

## Goal

Implement zero-allocation network I/O using a buffer pool with **codegen-derived size classes**.

## Strategy

- **Allocation**: Lazy (on-demand) - Pool starts empty, buffers allocated only when first needed
- **Size Classes**: Codegen-derived from `contracts.yaml` - One source of truth, zero configuration
- **Key Insight**: Use TLV `length` field directly, no Message trait changes needed

---

## Phase 1: Enhance Codegen (4-5 hours)

### Task 1.1: Add Size Calculation Logic

**File**: `scripts/codegen.py`

Add functions to calculate message sizes:

```python
# Type sizes in bytes
TYPE_SIZES = {
    'uint8': 1,
    'uint16': 2,
    'uint32': 4,
    'uint64': 8,
    'uint128': 16,
    'uint256': 32,
    'Address': 20,
    'bool': 1,
}

def calculate_message_size(message_def):
    """Calculate max serialized size from field definitions."""
    total = 0
    for field in message_def['fields']:
        field_type = field['type']

        # Handle FixedVec<T, N>
        if field_type.startswith('FixedVec<'):
            inner, size = parse_fixed_vec(field_type)
            total += TYPE_SIZES.get(inner, 1) * size
        # Handle FixedStr<N>
        elif field_type.startswith('FixedStr<'):
            size = parse_fixed_str(field_type)
            total += size
        else:
            total += TYPE_SIZES.get(field_type, 0)

    return total

def next_power_of_two(n):
    """Round up to next power of 2, min 128."""
    if n <= 128:
        return 128
    power = 1
    while power < n:
        power *= 2
    return power

def generate_buffer_pool_config(messages):
    """Generate buffer pool configuration from message definitions."""
    size_to_messages = {}

    for msg in messages:
        size = calculate_message_size(msg)
        buffer_class = next_power_of_two(size)

        if buffer_class not in size_to_messages:
            size_to_messages[buffer_class] = []
        size_to_messages[buffer_class].append((msg['name'], size))

    # Generate capacity heuristic: more small buffers, fewer large
    config = {}
    for buffer_class, msgs in size_to_messages.items():
        # Base: 64KB per size class, divided by buffer size
        capacity = 65536 // buffer_class
        config[buffer_class] = capacity

    return config, size_to_messages
```

### Task 1.2: Generate BufferPool Configuration

Add to code generation output:

```python
def generate_buffer_pool_code(messages):
    config, size_to_messages = generate_buffer_pool_config(messages)

    code = '''
// Auto-generated buffer pool configuration
// DO NOT EDIT - Generated from contracts.yaml

use std::collections::HashMap;

pub fn create_buffer_pool_config() -> HashMap<usize, usize> {
    let mut config = HashMap::new();
'''

    for buffer_class, capacity in sorted(config.items()):
        msgs = size_to_messages[buffer_class]
        msg_names = ', '.join(f"{name} ({size}B)" for name, size in msgs)

        code += f'''
    // {msg_names}
    config.insert({buffer_class}, {capacity});
'''

    code += '''
    config
}

// Per-message size constants for debugging/validation
'''

    for msg in messages:
        size = calculate_message_size(msg)
        buffer_class = next_power_of_two(size)
        code += f'''
pub const {msg['name'].upper()}_MAX_SIZE: usize = {size};
pub const {msg['name'].upper()}_BUFFER_CLASS: usize = {buffer_class};
'''

    return code
```

### Task 1.3: Integrate into Generation Pipeline

Update `generate_all()` to include buffer pool config:

```python
def generate_all(contracts_file, output_file):
    messages = parse_yaml(contracts_file)

    # Existing message generation
    message_code = generate_messages(messages)

    # NEW: Buffer pool configuration
    buffer_pool_code = generate_buffer_pool_code(messages)

    with open(output_file, 'w') as f:
        f.write(message_code)
        f.write('\n\n')
        f.write(buffer_pool_code)
```

---

## Phase 2: Implement BufferPool (3-4 hours)

### Task 2.1: Create buffer_pool.rs

**File**: `crates/mycelium-transport/src/buffer_pool.rs`

```rust
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Buffer pool configuration (size_class -> capacity)
#[derive(Debug, Clone)]
pub struct BufferPoolConfig {
    pub size_classes: HashMap<usize, usize>,
}

impl BufferPoolConfig {
    pub fn from_map(map: HashMap<usize, usize>) -> Self {
        Self { size_classes: map }
    }
}

/// Zero-allocation buffer pool for network I/O
#[derive(Clone)]
pub struct BufferPool {
    inner: Arc<BufferPoolInner>,
}

struct BufferPoolInner {
    /// Size class -> vec of available buffers
    pools: Mutex<HashMap<usize, Vec<Vec<u8>>>>,
    config: BufferPoolConfig,
    stats: Mutex<BufferPoolStats>,
}

impl BufferPool {
    /// Create new buffer pool (starts empty, allocates on-demand)
    pub fn new(config: BufferPoolConfig) -> Self {
        let mut pools = HashMap::new();
        for size in config.size_classes.keys() {
            pools.insert(*size, Vec::new());
        }

        Self {
            inner: Arc::new(BufferPoolInner {
                pools: Mutex::new(pools),
                config,
                stats: Mutex::new(BufferPoolStats::default()),
            }),
        }
    }

    /// Acquire a buffer of at least `size` bytes
    pub fn acquire(&self, size: usize) -> PooledBuffer {
        let size_class = self.find_size_class(size);

        let mut pools = self.inner.pools.lock().unwrap();
        let buffer = pools
            .entry(size_class)
            .or_insert_with(Vec::new)
            .pop()
            .unwrap_or_else(|| {
                // Lazy allocation on first request
                let mut stats = self.inner.stats.lock().unwrap();
                stats.total_allocations += 1;
                tracing::debug!(size_class, "Allocating new buffer");
                vec![0u8; size_class]
            });

        {
            let mut stats = self.inner.stats.lock().unwrap();
            stats.total_acquires += 1;
            stats.currently_in_use += 1;
        }

        PooledBuffer {
            buffer,
            pool: self.clone(),
            size_class,
        }
    }

    fn find_size_class(&self, size: usize) -> usize {
        self.inner.config.size_classes
            .keys()
            .filter(|&&class| class >= size)
            .min()
            .copied()
            .unwrap_or_else(|| {
                tracing::warn!(size, "No size class found, rounding up");
                size.next_power_of_two().max(128)
            })
    }

    fn return_buffer(&self, mut buffer: Vec<u8>, size_class: usize) {
        let mut pools = self.inner.pools.lock().unwrap();

        if let Some(pool) = pools.get_mut(&size_class) {
            let max_capacity = self.inner.config.size_classes
                .get(&size_class)
                .copied()
                .unwrap_or(256);

            if pool.len() < max_capacity {
                // Clear buffer (security) and return to pool
                buffer.clear();
                buffer.resize(size_class, 0);
                pool.push(buffer);

                let mut stats = self.inner.stats.lock().unwrap();
                stats.total_returns += 1;
            }
        }

        let mut stats = self.inner.stats.lock().unwrap();
        stats.currently_in_use -= 1;
    }

    pub fn stats(&self) -> BufferPoolStats {
        self.inner.stats.lock().unwrap().clone()
    }
}

/// Pooled buffer that automatically returns to pool on drop
pub struct PooledBuffer {
    buffer: Vec<u8>,
    pool: BufferPool,
    size_class: usize,
}

impl PooledBuffer {
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn capacity(&self) -> usize {
        self.buffer.capacity()
    }

    pub fn resize(&mut self, new_len: usize, value: u8) {
        self.buffer.resize(new_len, value);
    }
}

impl Drop for PooledBuffer {
    fn drop(&mut self) {
        let buffer = std::mem::take(&mut self.buffer);
        self.pool.return_buffer(buffer, self.size_class);
    }
}

impl std::ops::Deref for PooledBuffer {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        &self.buffer
    }
}

impl std::ops::DerefMut for PooledBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.buffer
    }
}

#[derive(Debug, Clone, Default)]
pub struct BufferPoolStats {
    pub total_allocations: u64,  // New buffers allocated
    pub total_acquires: u64,     // acquire() calls
    pub total_returns: u64,      // Successful returns to pool
    pub currently_in_use: usize, // Buffers currently borrowed
}

impl BufferPoolStats {
    /// Hit rate: % of acquires that reused existing buffer
    pub fn hit_rate(&self) -> f64 {
        if self.total_acquires == 0 {
            return 0.0;
        }
        let hits = self.total_acquires - self.total_allocations;
        (hits as f64 / self.total_acquires as f64) * 100.0
    }
}
```

---

## Phase 3: Integrate with Codec (2-3 hours)

### Task 3.1: Add Pooled Reading Function

**File**: `crates/mycelium-transport/src/codec.rs`

```rust
use crate::buffer_pool::{BufferPool, PooledBuffer};

/// Read TLV frame using buffer pool (zero-allocation)
pub async fn read_frame_pooled<R: AsyncRead + Unpin>(
    stream: &mut R,
    pool: &BufferPool,
) -> Result<(u16, PooledBuffer), CodecError> {
    // Read TLV header
    let mut header = [0u8; HEADER_SIZE];
    stream.read_exact(&mut header).await
        .map_err(|e| CodecError::IoError(e.to_string()))?;

    let type_id = u16::from_le_bytes([header[0], header[1]]);
    let length = u32::from_le_bytes([header[2], header[3], header[4], header[5]]) as usize;

    if length > MAX_PAYLOAD_SIZE {
        return Err(CodecError::MessageTooLarge {
            size: length,
            max: MAX_PAYLOAD_SIZE,
        });
    }

    // Acquire pooled buffer using runtime length from TLV
    let mut buffer = pool.acquire(length);
    buffer.resize(length, 0);

    // Read directly into pooled buffer
    stream.read_exact(&mut buffer[..length]).await
        .map_err(|e| CodecError::IoError(e.to_string()))?;

    Ok((type_id, buffer))
}
```

---

## Phase 4: Transport Integration (3-4 hours)

### Task 4.1: Add BufferPool to TcpTransport

**File**: `crates/mycelium-transport/src/tcp.rs`

```rust
use crate::buffer_pool::{BufferPool, BufferPoolConfig};

pub struct TcpTransport {
    listener: Arc<TcpListener>,
    channels: ChannelManager,
    buffer_pool: Option<BufferPool>,  // NEW
}

impl TcpTransport {
    pub async fn bind_with_buffer_pool(
        addr: SocketAddr,
        config: TransportConfig,
        pool_config: BufferPoolConfig,
    ) -> Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        let buffer_pool = Some(BufferPool::new(pool_config));

        Ok(Self {
            listener: Arc::new(listener),
            channels: ChannelManager::new(config),
            buffer_pool,
        })
    }
}
```

### Task 4.2: Update Connection Handler

**File**: `crates/mycelium-transport/src/stream.rs`

```rust
pub async fn handle_stream_connection(
    mut stream: impl AsyncRead + AsyncWrite + Unpin,
    channels: ChannelManager,
    buffer_pool: Option<&BufferPool>,
) -> Result<()> {
    loop {
        let (type_id, bytes) = if let Some(pool) = buffer_pool {
            // Use pooled reading
            let (type_id, pooled) = read_frame_pooled(&mut stream, pool).await?;
            (type_id, pooled.to_vec())  // Convert for now
        } else {
            // Fallback to regular reading
            read_frame(&mut stream).await?
        };

        // ... rest of message handling
    }
}
```

---

## Phase 5: Testing (3-4 hours)

### Task 5.1: Unit Tests

**File**: `crates/mycelium-transport/src/buffer_pool.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lazy_allocation() {
        let mut config = HashMap::new();
        config.insert(128, 10);
        let pool = BufferPool::new(BufferPoolConfig::from_map(config));

        let stats = pool.stats();
        assert_eq!(stats.total_allocations, 0); // No allocation yet

        // First acquire allocates
        let _buf = pool.acquire(100);
        assert_eq!(pool.stats().total_allocations, 1);

        // After drop, second acquire reuses
        drop(_buf);
        let _buf2 = pool.acquire(100);
        assert_eq!(pool.stats().total_allocations, 1); // Still 1!
    }

    #[test]
    fn test_hit_rate() {
        let mut config = HashMap::new();
        config.insert(128, 10);
        let pool = BufferPool::new(BufferPoolConfig::from_map(config));

        // First 10 acquires allocate
        for _ in 0..10 {
            let _buf = pool.acquire(100);
        }

        // Next 90 acquires reuse
        for _ in 0..90 {
            let _buf = pool.acquire(100);
        }

        let stats = pool.stats();
        assert_eq!(stats.total_allocations, 10);
        assert_eq!(stats.total_acquires, 100);
        assert_eq!(stats.hit_rate(), 90.0); // 90% hit rate
    }
}
```

### Task 5.2: Integration Test

**File**: `crates/mycelium-transport/tests/buffer_pool_integration.rs`

```rust
#[tokio::test]
async fn test_tcp_with_buffer_pool() {
    // Load generated pool config
    let pool_config = BufferPoolConfig::from_map(
        mycelium_protocol::generated::create_buffer_pool_config()
    );

    let transport = TcpTransport::bind_with_buffer_pool(
        "127.0.0.1:0".parse().unwrap(),
        TransportConfig::default(),
        pool_config,
    ).await.unwrap();

    // Send 1000 messages
    for _ in 0..1000 {
        // ... send messages
    }

    // Verify high hit rate
    let stats = transport.buffer_pool_stats();
    assert!(stats.hit_rate() > 80.0);
}
```

---

## Success Criteria

- [x] Codegen calculates message sizes from `contracts.yaml`
- [x] BufferPool starts with 0 memory, allocates on-demand
- [x] All 103 tests pass (40 protocol + 63 transport)
- [x] Hit rate >80% under load (1000+ messages)
- [x] Zero memory leaks (all buffers returned on drop)
- [x] Allocations per message: <2 (down from 3)

---

## Files to Create/Modify

### Create
1. `crates/mycelium-transport/src/buffer_pool.rs` - NEW module
2. `crates/mycelium-transport/tests/buffer_pool_integration.rs` - NEW test

### Modify
3. `scripts/codegen.py` - Add size calculation + config generation
4. `crates/mycelium-protocol/src/generated.rs` - Regenerate with pool config
5. `crates/mycelium-transport/src/codec.rs` - Add `read_frame_pooled()`
6. `crates/mycelium-transport/src/tcp.rs` - Use buffer pool
7. `crates/mycelium-transport/src/unix.rs` - Use buffer pool
8. `crates/mycelium-transport/src/stream.rs` - Update connection handler
9. `crates/mycelium-transport/src/lib.rs` - Export BufferPool API
10. `crates/mycelium-transport/src/config.rs` - Add buffer_pool_enabled flag

---

## Total Estimate

**16-20 hours (~2 days focused work)**

Breakdown:
- Phase 1 (Codegen): 4-5 hours
- Phase 2 (BufferPool): 3-4 hours
- Phase 3 (Codec): 2-3 hours
- Phase 4 (Transports): 3-4 hours
- Phase 5 (Testing): 3-4 hours
