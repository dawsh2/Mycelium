use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Buffer pool configuration (size_class -> capacity)
#[derive(Debug, Clone)]
pub struct BufferPoolConfig {
    pub size_classes: HashMap<usize, usize>,
}

impl BufferPoolConfig {
    /// Create buffer pool config from a HashMap (typically from codegen)
    pub fn from_map(map: HashMap<usize, usize>) -> Self {
        Self { size_classes: map }
    }
}

/// Zero-allocation buffer pool for network I/O
///
/// Starts empty (lazy allocation) and allocates buffers on-demand.
/// Buffers are automatically returned to the pool when dropped.
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
    ///
    /// # Example
    /// ```ignore
    /// use mycelium_protocol::create_buffer_pool_config;
    /// use crate::BufferPool;
    ///
    /// let config = create_buffer_pool_config();
    /// let pool = BufferPool::new(BufferPoolConfig::from_map(config));
    /// ```
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
    ///
    /// Returns a `PooledBuffer` that automatically returns to the pool on drop.
    /// If no buffer is available, allocates a new one (lazy allocation).
    pub fn acquire(&self, size: usize) -> PooledBuffer {
        let size_class = self.find_size_class(size);

        // Track whether we allocated a new buffer
        let mut allocated_new = false;

        // Acquire buffer from pool (or allocate)
        let buffer = {
            let mut pools = self.inner.pools.lock().unwrap();
            pools
                .entry(size_class)
                .or_insert_with(Vec::new)
                .pop()
                .unwrap_or_else(|| {
                    // Lazy allocation on first request
                    allocated_new = true;
                    tracing::debug!(size_class, "Allocating new buffer");
                    vec![0u8; size_class]
                })
        }; // Release pools lock before acquiring stats lock

        // Update stats with a single lock acquisition
        {
            let mut stats = self.inner.stats.lock().unwrap();
            if allocated_new {
                stats.total_allocations += 1;
            }
            stats.total_acquires += 1;
            stats.currently_in_use += 1;
        }

        PooledBuffer {
            buffer,
            pool: self.clone(),
            size_class,
        }
    }

    /// Find the smallest size class that can hold `size` bytes
    fn find_size_class(&self, size: usize) -> usize {
        self.inner
            .config
            .size_classes
            .keys()
            .filter(|&&class| class >= size)
            .min()
            .copied()
            .unwrap_or_else(|| {
                tracing::warn!(size, "No size class found, rounding up");
                size.next_power_of_two().max(128)
            })
    }

    /// Return a buffer to the pool
    fn return_buffer(&self, mut buffer: Vec<u8>, size_class: usize) {
        // Track whether buffer was returned to pool
        let mut buffer_returned = false;

        // Acquire pools lock and do all pool operations
        {
            let mut pools = self.inner.pools.lock().unwrap();

            if let Some(pool) = pools.get_mut(&size_class) {
                let max_capacity = self
                    .inner
                    .config
                    .size_classes
                    .get(&size_class)
                    .copied()
                    .unwrap_or(256);

                if pool.len() < max_capacity {
                    // Clear buffer (security) and return to pool
                    buffer.clear();
                    buffer.resize(size_class, 0);
                    pool.push(buffer);
                    buffer_returned = true;
                }
            }
        } // Release pools lock before acquiring stats lock

        // Update stats with a single lock acquisition
        {
            let mut stats = self.inner.stats.lock().unwrap();
            if buffer_returned {
                stats.total_returns += 1;
            }
            stats.currently_in_use -= 1;
        }
    }

    /// Get current buffer pool statistics
    pub fn stats(&self) -> BufferPoolStats {
        self.inner.stats.lock().unwrap().clone()
    }
}

/// Pooled buffer that automatically returns to pool on drop
///
/// Implements `Deref` and `DerefMut` for transparent byte slice access.
pub struct PooledBuffer {
    buffer: Vec<u8>,
    pool: BufferPool,
    size_class: usize,
}

impl PooledBuffer {
    /// Get the current length of the buffer
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Get the capacity of the buffer
    pub fn capacity(&self) -> usize {
        self.buffer.capacity()
    }

    /// Resize the buffer to a new length
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

/// Buffer pool statistics for monitoring and debugging
#[derive(Debug, Clone, Default)]
pub struct BufferPoolStats {
    /// Total number of new buffers allocated
    pub total_allocations: u64,
    /// Total number of acquire() calls
    pub total_acquires: u64,
    /// Total number of successful returns to pool
    pub total_returns: u64,
    /// Number of buffers currently borrowed
    pub currently_in_use: usize,
}

impl BufferPoolStats {
    /// Calculate hit rate: percentage of acquires that reused existing buffer
    ///
    /// Returns 0.0 if no acquires have been made yet.
    pub fn hit_rate(&self) -> f64 {
        if self.total_acquires == 0 {
            return 0.0;
        }
        let hits = self.total_acquires - self.total_allocations;
        (hits as f64 / self.total_acquires as f64) * 100.0
    }
}

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
    fn test_buffer_reuse() {
        let mut config = HashMap::new();
        config.insert(128, 10);
        let pool = BufferPool::new(BufferPoolConfig::from_map(config));

        // Acquire and release
        {
            let _buf = pool.acquire(100);
        }

        let stats = pool.stats();
        assert_eq!(stats.total_allocations, 1);
        assert_eq!(stats.total_acquires, 1);
        assert_eq!(stats.total_returns, 1);
        assert_eq!(stats.currently_in_use, 0);

        // Second acquire should reuse
        let _buf2 = pool.acquire(100);
        let stats2 = pool.stats();
        assert_eq!(stats2.total_allocations, 1); // No new allocation
        assert_eq!(stats2.total_acquires, 2);
        assert_eq!(stats2.currently_in_use, 1);
    }

    #[test]
    fn test_hit_rate() {
        let mut config = HashMap::new();
        config.insert(128, 10);
        let pool = BufferPool::new(BufferPoolConfig::from_map(config));

        // First 10 acquires allocate (keep them alive)
        let mut buffers = Vec::new();
        for _ in 0..10 {
            buffers.push(pool.acquire(100));
        }
        assert_eq!(pool.stats().total_allocations, 10);

        // Drop all buffers, returning them to pool
        buffers.clear();

        // Next 90 acquires should reuse existing buffers
        for _ in 0..90 {
            let _buf = pool.acquire(100);
        }

        let stats = pool.stats();
        assert_eq!(stats.total_allocations, 10);
        assert_eq!(stats.total_acquires, 100);
        assert_eq!(stats.hit_rate(), 90.0); // 90% hit rate
    }

    #[test]
    fn test_size_class_selection() {
        let mut config = HashMap::new();
        config.insert(128, 10);
        config.insert(256, 10);
        config.insert(512, 10);
        let pool = BufferPool::new(BufferPoolConfig::from_map(config));

        // 100 bytes should use 128 class
        let buf1 = pool.acquire(100);
        assert_eq!(buf1.len(), 128);

        // 200 bytes should use 256 class
        let buf2 = pool.acquire(200);
        assert_eq!(buf2.len(), 256);

        // 300 bytes should use 512 class
        let buf3 = pool.acquire(300);
        assert_eq!(buf3.len(), 512);
    }

    #[test]
    fn test_capacity_limits() {
        let mut config = HashMap::new();
        config.insert(128, 2); // Max 2 buffers
        let pool = BufferPool::new(BufferPoolConfig::from_map(config));

        // Allocate 3 buffers simultaneously
        let buf1 = pool.acquire(100);
        let buf2 = pool.acquire(100);
        let buf3 = pool.acquire(100);

        assert_eq!(pool.stats().total_allocations, 3);

        // Drop all buffers
        drop(buf1);
        drop(buf2);
        drop(buf3);

        let stats = pool.stats();
        assert_eq!(stats.total_allocations, 3);
        assert_eq!(stats.total_returns, 2); // Only 2 returned to pool (capacity limit)
        assert_eq!(stats.currently_in_use, 0);
    }

    #[test]
    fn test_concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let mut config = HashMap::new();
        config.insert(128, 100);
        let pool = Arc::new(BufferPool::new(BufferPoolConfig::from_map(config)));

        let mut handles = vec![];

        // Spawn 10 threads, each acquiring 10 buffers
        for _ in 0..10 {
            let pool_clone = Arc::clone(&pool);
            let handle = thread::spawn(move || {
                for _ in 0..10 {
                    let _buf = pool_clone.acquire(100);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let stats = pool.stats();
        assert_eq!(stats.total_acquires, 100);
        assert!(stats.hit_rate() > 0.0); // Should have some reuse
    }

    #[test]
    fn test_pooled_buffer_operations() {
        let mut config = HashMap::new();
        config.insert(128, 10);
        let pool = BufferPool::new(BufferPoolConfig::from_map(config));

        let mut buf = pool.acquire(64);
        assert_eq!(buf.len(), 128);
        assert!(!buf.is_empty());
        assert_eq!(buf.capacity(), 128);

        // Test resize
        buf.resize(64, 0);
        assert_eq!(buf.len(), 64);

        // Test deref
        buf[0] = 42;
        assert_eq!(buf[0], 42);
    }

    #[test]
    fn test_zero_size_request() {
        let mut config = HashMap::new();
        config.insert(128, 10);
        let pool = BufferPool::new(BufferPoolConfig::from_map(config));

        let buf = pool.acquire(0);
        assert_eq!(buf.len(), 128); // Still gets smallest available size class
    }

    #[test]
    fn test_very_large_request() {
        let mut config = HashMap::new();
        config.insert(128, 10);
        config.insert(256, 10);
        let pool = BufferPool::new(BufferPoolConfig::from_map(config));

        // Request larger than all size classes
        let buf = pool.acquire(1024);
        assert_eq!(buf.len(), 1024); // Falls back to exact allocation
    }

    #[test]
    fn test_empty_config() {
        let config = HashMap::new();
        let pool = BufferPool::new(BufferPoolConfig::from_map(config));

        // Should still work, just no pooling (allocates next power of 2)
        let buf = pool.acquire(100);
        assert_eq!(buf.len(), 128); // Rounds up to next power of 2

        let stats = pool.stats();
        assert_eq!(stats.total_allocations, 1);
        assert_eq!(stats.hit_rate(), 0.0); // No hits without pool
    }

    #[test]
    fn test_stats_initially_zero() {
        let mut config = HashMap::new();
        config.insert(128, 10);
        let pool = BufferPool::new(BufferPoolConfig::from_map(config));

        let stats = pool.stats();
        assert_eq!(stats.total_allocations, 0);
        assert_eq!(stats.total_acquires, 0);
        assert_eq!(stats.total_returns, 0);
        assert_eq!(stats.currently_in_use, 0);
        assert_eq!(stats.hit_rate(), 0.0);
    }

    #[test]
    fn test_rapid_acquire_release() {
        let mut config = HashMap::new();
        config.insert(128, 5);
        let pool = BufferPool::new(BufferPoolConfig::from_map(config));

        // Rapidly acquire and release to test pool stability
        for _ in 0..1000 {
            let _buf = pool.acquire(100);
            // Immediately dropped, returning to pool
        }

        let stats = pool.stats();
        assert_eq!(stats.total_acquires, 1000);
        assert_eq!(stats.currently_in_use, 0);
        assert!(stats.hit_rate() > 99.0); // Should have very high hit rate
    }

    #[test]
    fn test_config_from_map() {
        let mut map = HashMap::new();
        map.insert(128, 10);
        map.insert(256, 20);
        map.insert(512, 15);

        let config = BufferPoolConfig::from_map(map);
        let pool = BufferPool::new(config);

        // Verify all size classes work
        let buf128 = pool.acquire(100);
        let buf256 = pool.acquire(200);
        let buf512 = pool.acquire(400);

        assert_eq!(buf128.len(), 128);
        assert_eq!(buf256.len(), 256);
        assert_eq!(buf512.len(), 512);
    }
}
