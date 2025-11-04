# Code Review Action Items

**Priority-ordered list of issues to address from the code review**

---

## 🚨 CRITICAL - Fix Immediately (Blocks Development)

### 1. Add Missing MAX_POOL_ADDRESSES Constant
**File:** `crates/mycelium-protocol/src/fixed_vec.rs`
**Issue:** Build fails with `unresolved import MAX_POOL_ADDRESSES`
**Fix:**
```rust
// Add after line 28 (after MAX_SYMBOL_LENGTH):
pub const MAX_POOL_ADDRESSES: usize = 10;  // Maximum addresses in arbitrage path
```
**Impact:** Blocks all compilation and testing
**Time:** 5 minutes

---

### 2. Fix or Remove Disabled Modules
**Files:**
- `crates/mycelium-transport/src/lib.rs:25-27` (commented out)
- `crates/mycelium-transport/src/backpressure.rs`
- `crates/mycelium-transport/src/pool.rs`

**Issue:** Modules commented out with "TODO: Fix compilation errors"

**Option A - Fix:**
1. Add `Timeout` variant to `TransportError` enum
2. Re-enable modules in lib.rs
3. Run tests to verify

**Option B - Defer:**
1. Move to `experimental/` directory
2. Document as "Phase 4 - Future Work"
3. Remove from current lib.rs

**Impact:** Prevents use of advanced features, unclear codebase status
**Time:** 2-4 hours to fix, or 30 minutes to defer

---

### 3. Verify Test Suite Runs
**Command:** `cargo test`
**Issue:** Cannot run tests due to build errors
**Steps:**
1. Fix #1 above (MAX_POOL_ADDRESSES)
2. Run `cargo test --all`
3. Document any failures
4. Fix critical test failures

**Impact:** No way to verify correctness
**Time:** 1 hour after #1 is fixed

---

## ⚠️ HIGH PRIORITY - Address This Week

### 4. Replace .expect() in Production Code
**Files:** Multiple (38 occurrences in src/)
**Issue:** Will panic in production instead of propagating errors

**Pattern to replace:**
```rust
// BEFORE (will panic):
let pub_ = self.unix_publisher(&target_node.name)
    .await
    .expect("Failed to create Unix publisher");

// AFTER (proper error handling):
let pub_ = self.unix_publisher(&target_node.name)
    .await
    .ok_or_else(|| TransportError::ServiceNotFound(
        format!("Failed to create Unix publisher to '{}'", target_node.name)
    ))?;
```

**Files to check:**
- `crates/mycelium-transport/src/bus.rs:640, 732`
- `crates/mycelium-transport/src/tcp.rs:231`
- Others found via: `grep -r "\.expect(" crates/mycelium-transport/src/*.rs`

**Impact:** Production panics instead of graceful error handling
**Time:** 3-4 hours

---

### 5. Expand TransportError Enum
**File:** `crates/mycelium-transport/src/error.rs`
**Issue:** Error types too generic

**Add these variants:**
```rust
#[derive(Error, Debug)]
pub enum TransportError {
    // ... existing variants ...

    #[error("Operation timed out after {0:?}")]
    Timeout(Duration),

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("Resource exhausted: {0}")]
    ResourceExhausted(String),

    #[error("Message too large: {size} bytes (max: {max})")]
    MessageTooLarge { size: usize, max: usize },
}
```

**Impact:** Better error diagnostics, enables timeout handling
**Time:** 30 minutes

---

### 6. Document Concurrency Guarantees
**File:** Create `docs/CONCURRENCY.md`
**Issue:** No documentation of lock ordering or thread safety

**Content should cover:**
- Lock acquisition order to prevent deadlocks
- Which structures are `Send`/`Sync`
- Thread-safety guarantees for each component
- When to use RwLock vs Mutex vs Atomic

**Example:**
```markdown
## Lock Ordering

To prevent deadlocks, always acquire locks in this order:

1. `MessageBus::tcp_transports` (RwLock)
2. `MessageBus::unix_transports` (RwLock)
3. Service-specific locks

Never acquire locks in reverse order.
```

**Impact:** Prevents future deadlock bugs
**Time:** 2 hours

---

## 📋 MEDIUM PRIORITY - Next Sprint

### 7. Add Missing Core Documentation
**Files to create:**
- `CONTRIBUTING.md` - How to contribute
- `CHANGELOG.md` - Version history
- `MIGRATION.md` - Upgrading guide
- `SECURITY.md` - Security policy

**Templates available at:** https://github.com/templates

**Impact:** Easier onboarding, better OSS practices
**Time:** 3 hours

---

### 8. Improve Test Coverage
**Current:** Unknown (tests don't run)
**Target:** 70%+ for core modules

**Steps:**
1. Run `cargo install cargo-tarpaulin` or `cargo-llvm-cov`
2. Generate coverage report: `cargo tarpaulin --out Html`
3. Identify uncovered code paths
4. Add tests for critical paths

**Focus areas:**
- Error handling paths
- Edge cases in message validation
- Concurrent scenarios
- Transport failures

**Impact:** Higher confidence in correctness
**Time:** 1 week

---

### 9. Add Remote Transport Benchmarks
**File:** Create `benches/remote_transports.rs`
**Issue:** Only local transport benchmarked

**Benchmarks needed:**
- TCP transport end-to-end latency
- Unix socket transport latency
- Message encoding/decoding overhead
- Subscriber filtering performance

**Impact:** Understand performance characteristics of all deployment modes
**Time:** 1 day

---

### 10. Optimize Topic Storage
**Files:** `crates/mycelium-protocol/src/envelope.rs`
**Issue:** `topic: String` causes heap allocation per message

**Optimization:**
```rust
pub struct Envelope {
    type_id: u16,
    topic: Arc<str>,  // ← Change from String to Arc<str>
    // ... other fields
}
```

**Benefits:**
- Shared topics don't allocate
- Lower memory usage for fanout
- Better cache locality

**Trade-offs:**
- Topic mutation becomes cloning
- Small code complexity increase

**Impact:** 5-10% memory reduction for high-fanout scenarios
**Time:** 2-3 hours

---

## ✨ NICE TO HAVE - Future Enhancements

### 11. Rate Limiting for Publishers
**Feature:** Prevent DoS from fast publishers

**Config addition:**
```rust
pub struct TransportConfig {
    pub max_publish_rate: Option<u64>,  // messages per second
    // ...
}
```

**Time:** 1 day

---

### 12. Implement Request/Reply Pattern
**Feature:** Correlation IDs for RPC-style communication

**Status:** Envelope fields exist, implementation stubbed

**Time:** 3-4 days

---

### 13. Complete Actor Hierarchy
**Feature:** Parent/child supervision with escalation

**Status:** Basic supervision works, hierarchy not implemented

**Time:** 1 week

---

### 14. Message Size Limits
**Feature:** Enforce max message size at bus level

**Config addition:**
```rust
pub struct TransportConfig {
    pub max_message_size: usize,  // bytes
    // ...
}
```

**Time:** 1 day

---

### 15. Migration Guide
**File:** `docs/MIGRATION.md`
**Content:** Guide for upgrading 0.1.x → 0.2.0

**Time:** 2 hours (after 0.2.0 API is finalized)

---

## Summary Timeline

**Week 1 (Critical):**
- Day 1: Fix #1, #2, #3 (build and tests)
- Day 2-3: Fix #4 (remove .expect())
- Day 4: Fix #5, #6 (error types, concurrency docs)

**Week 2 (High Priority):**
- Day 1-2: #7 (documentation)
- Day 3-5: #8 (test coverage)

**Week 3 (Medium Priority):**
- Day 1: #9 (benchmarks)
- Day 2-3: #10 (optimizations)
- Day 4-5: Buffer, prepare release

**Week 4 (Nice to Have):**
- Select 2-3 items from #11-15 based on priorities

---

## Success Metrics

**After Week 1:**
- [ ] Build succeeds with zero errors
- [ ] All tests pass
- [ ] No .expect() in production code paths

**After Week 2:**
- [ ] 70%+ test coverage
- [ ] All core documentation complete
- [ ] Concurrency guarantees documented

**After Week 3:**
- [ ] Performance benchmarks for all transports
- [ ] Optimizations applied
- [ ] Ready for 0.2.0-alpha release

---

## Questions Before Starting?

1. Should disabled modules be fixed or deferred?
2. What's the target release date?
3. Are there specific deployment scenarios to prioritize?
4. What's the acceptable test coverage threshold?

---

**Last Updated:** 2025-01-04
**Review Document:** `docs/CODE_REVIEW.md`
