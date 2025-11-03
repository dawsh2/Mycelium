# Guardrails

**Philosophy**: Code is reality. CI enforces quality gates. No warnings, no duplication, no merge.

---

## Active Guardrails

### 1. Code Duplication Detection ✅
**Purpose**: Prevent LLM-assisted copy-paste patterns

**Tool**: `scripts/detect_duplication/`
- AST-based semantic analysis using syn crate
- Threshold: 70% similarity = CI failure
- Compares: signature (30%) + body structure (50%) + tokens (20%)

**Run locally**:
```bash
cd scripts/detect_duplication
cargo build --release
./target/release/detect_duplication --path ../../crates --threshold 0.70
```

**CI**: `.github/workflows/ci.yml` - `duplication` job

---

### 2. Test Suite ✅
**Standards**:
- All tests must pass
- Doc tests must compile

**Run locally**:
```bash
cargo test --workspace
cargo test --doc --workspace
```

**CI**: `test` job

---

### 3. Clippy Lints ✅
**Standards**:
- `RUSTFLAGS="-D warnings"` - warnings are errors
- Standard clippy lints enforced

**Run locally**:
```bash
cargo clippy --workspace --all-features -- -D warnings
```

**CI**: `clippy` job

---

### 4. Format Check ✅
**Standard**: rustfmt default config

**Run locally**:
```bash
cargo fmt --all -- --check
```

**CI**: `fmt` job

---

## CI Pipeline

**File**: `.github/workflows/ci.yml`

**Jobs** (all must pass):
1. `test` - Full test suite + doc tests
2. `fmt` - Format check
3. `clippy` - Lint validation
4. `duplication` - Duplication detection (70% threshold)
5. `build` - Release build check

**Trigger**: Every push to main/develop, every PR

---

## Deferred (Intentionally)

### system.yaml Validation
**Status**: Waiting for 3+ services before adding

**Rationale**: Current scope (transport + protocol) doesn't warrant architectural specification overhead. Will add when service boundaries become unclear.

**Trigger**: Add when we have 3+ services with complex dependencies

---

## Success Metrics

- ✅ Zero code duplication in main branch (>70% similarity)
- ✅ Zero compiler warnings
- ✅ All tests passing
- ✅ Clean CI on every merge
