# Buffer Pool Implementation Checklist

Track progress through each phase of implementation.

---

## Phase 1: Enhance Codegen ⏳

### Codegen Logic
- [ ] Add `TYPE_SIZES` dictionary to `codegen.py`
- [ ] Implement `calculate_message_size(message_def)`
- [ ] Implement `next_power_of_two(n)`
- [ ] Implement `generate_buffer_pool_config(messages)`
- [ ] Implement `generate_buffer_pool_code(messages)`
- [ ] Integrate into `generate_all()` pipeline

### Validation
- [ ] Run codegen on `contracts.yaml`
- [ ] Verify `generated.rs` contains size constants
- [ ] Verify `create_buffer_pool_config()` function generated
- [ ] Check size calculations are correct (manual inspection)

---

## Phase 2: Implement BufferPool ⏳

### Core Implementation
- [ ] Create `buffer_pool.rs` module
- [ ] Implement `BufferPoolConfig` struct
- [ ] Implement `BufferPool` struct with lazy allocation
- [ ] Implement `PooledBuffer` with Drop trait
- [ ] Implement `BufferPoolStats` tracking
- [ ] Implement `find_size_class()` logic

### Module Integration
- [ ] Add `pub mod buffer_pool;` to `lib.rs`
- [ ] Export `BufferPool`, `BufferPoolConfig`, `PooledBuffer`, `BufferPoolStats`
- [ ] Add `buffer_pool` feature flag (optional)

### Unit Tests
- [ ] Test: Lazy allocation (pool starts empty)
- [ ] Test: Buffer reuse (same size class)
- [ ] Test: Size class selection (correct rounding)
- [ ] Test: Capacity limits (pool doesn't grow unbounded)
- [ ] Test: Drop behavior (buffers returned)
- [ ] Test: Hit rate calculation
- [ ] Test: Concurrent access (multiple threads)

---

## Phase 3: Integrate with Codec ⏳

### Implementation
- [ ] Add `use buffer_pool::*` to `codec.rs`
- [ ] Implement `read_frame_pooled()` function
- [ ] Use TLV `length` field directly (not MAX_SIZE)
- [ ] Handle error cases (buffer too large, I/O errors)

### Testing
- [ ] Test: Read small message (<128 bytes)
- [ ] Test: Read medium message (128-256 bytes)
- [ ] Test: Read large message (>1KB)
- [ ] Test: Message too large error
- [ ] Test: Buffer properly returned on error

---

## Phase 4: Transport Integration ⏳

### TcpTransport
- [ ] Add `buffer_pool: Option<BufferPool>` field to `TcpTransport`
- [ ] Implement `bind_with_buffer_pool()` constructor
- [ ] Update existing `bind()` to use default config
- [ ] Pass pool to connection handler
- [ ] Add `buffer_pool_stats()` method

### UnixTransport
- [ ] Add `buffer_pool: Option<BufferPool>` field
- [ ] Implement `bind_with_buffer_pool()` constructor
- [ ] Update existing constructors
- [ ] Pass pool to connection handler

### Stream Handler
- [ ] Update `handle_stream_connection()` signature
- [ ] Use `read_frame_pooled()` when pool available
- [ ] Fall back to `read_frame()` when pool is None
- [ ] Update all call sites

### Configuration
- [ ] Add `buffer_pool_enabled: bool` to `TransportConfig`
- [ ] Default to `true` (enabled by default)
- [ ] Allow disabling for debugging

---

## Phase 5: Testing ⏳

### Unit Tests (buffer_pool.rs)
- [ ] All Phase 2 tests passing
- [ ] Code coverage >80%

### Integration Tests
- [ ] Create `tests/buffer_pool_integration.rs`
- [ ] Test: TCP transport with buffer pool
- [ ] Test: Unix transport with buffer pool
- [ ] Test: High-volume traffic (1000+ messages)
- [ ] Test: Verify hit rate >80%
- [ ] Test: Verify allocations <2 per message
- [ ] Test: Memory leak check (valgrind/miri if possible)

### Regression Tests
- [ ] All existing tests pass (40 protocol + 63 transport)
- [ ] No performance regressions
- [ ] Benchmark: Compare with/without pool

---

## Validation & Documentation ⏳

### Performance Validation
- [ ] Benchmark allocations per message: before vs after
- [ ] Benchmark latency impact: <50ns overhead
- [ ] Profile under load: verify hit rate >80%
- [ ] Memory usage: verify pool doesn't grow unbounded

### Documentation
- [ ] Update `docs/BUFFER_POOL.md` with actual results
- [ ] Add docstrings to all public APIs
- [ ] Add usage examples to `buffer_pool.rs`
- [ ] Update `README.md` if needed

### Code Review
- [ ] Self-review all changes
- [ ] Check for unsafe code (should be none)
- [ ] Check for unwrap() calls (should use proper error handling)
- [ ] Check for TODOs (should be resolved or documented)

---

## Completion Criteria ✅

All checkboxes must be complete before considering this done:

- [ ] Codegen automatically derives size classes from `contracts.yaml`
- [ ] BufferPool starts with 0 memory (lazy allocation verified)
- [ ] All 103+ tests pass
- [ ] Buffer pool hit rate >80% under load
- [ ] Zero memory leaks (verified with tests)
- [ ] Allocations per message <2 (down from 3)
- [ ] Performance overhead <50ns per message
- [ ] Documentation complete and accurate

---

## Notes

Add implementation notes, issues encountered, or decisions made here:

-

---

**Started**: [Date]
**Completed**: [Date]
**Total Time**: [Hours]
