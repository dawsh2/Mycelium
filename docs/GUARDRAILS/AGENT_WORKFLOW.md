# Agent Workflow for TDD + GUARDRAILS Development

**Purpose**: Formalize how to use Claude Code agents for spec-first, test-driven development with GUARDRAILS enforcement.

**Philosophy**: Spec → Red → Green → Review → Refactor (with agent orchestration)

---

## Overview: The Extended TDD Cycle

```
┌─────────────────────────────────────────────────────────────┐
│ Traditional TDD:  Red → Green → Refactor                    │
│ Our Workflow:     Spec → Red → Green → Review → Refactor    │
└─────────────────────────────────────────────────────────────┘

Phase 0: Task Setup        [scrum-leader + context-gathering]
    ↓
Phase 1: Spec              [general-purpose for system.yaml]
    ↓
Phase 2: Red (Tests)       [Manual or general-purpose]
    ↓
Phase 3: Green (Minimal)   [Manual implementation]
    ↓
Phase 4: Review            [code-review agent]
    ↓
Phase 5: Refactor          [Manual with tests green]
    ↓
Phase 6: Document          [service-documentation]
    ↓
Phase 7: Commit            [logging + git commit]
```

---

## Phase 0: Task Setup

**Goal**: Create task with proper context and dependencies

### When to Use
- Starting a new feature
- Beginning a sprint
- Breaking down a large task

### Agents Involved

**1. scrum-leader** (Primary)
```bash
/scrum-leader "Create task for actor mailbox implementation"
```

**What it does**:
- Creates task file in `tasks/`
- Sets up dependency graph (DAG)
- Assigns priority and estimates

**2. context-gathering** (Automatic if task lacks context)
```
Agent automatically invoked if task file doesn't have "Context Manifest" section
```

**What it does**:
- Reads task file
- Gathers relevant codebase context
- Adds "Context Manifest" section to task file
- Lists relevant files, dependencies, patterns

### Success Criteria
- ✅ Task file exists with clear acceptance criteria
- ✅ Context manifest populated with relevant files
- ✅ Dependencies identified in DAG
- ✅ Task marked as ready to start

### Example
```markdown
# tasks/implement-mailbox.md

## Objective
Implement bounded mailbox with priority queue support

## Acceptance Criteria
- [ ] Mailbox has configurable capacity
- [ ] FIFO ordering for same-priority messages
- [ ] High-priority messages jump queue
- [ ] Backpressure when full

## Context Manifest
**Relevant Files**:
- `crates/mycelium-core/src/actor.rs` (Actor trait)
- `crates/mycelium-core/src/mailbox.rs` (implement here)
- `docs/GUARDRAILS/system.yaml` (mailbox spec)

**Dependencies**:
- Tokio mpsc channel
- Priority queue data structure

**Patterns**:
- Bounded channels for backpressure
- Envelope pattern for message metadata
```

---

## Phase 1: Spec (Define Before Implement)

**Goal**: Define component in system.yaml with contracts and invariants

### Agents Involved

**general-purpose** (for complex specs)
```
Use Task tool with prompt:
"Add mailbox spec to docs/GUARDRAILS/system.yaml with:
- Responsibilities
- Guarantees (FIFO, backpressure, priority)
- Invariants
- Interface contract"
```

Or **manual** for simple specs.

### What to Define

```yaml
# docs/GUARDRAILS/system.yaml

components:
  mailbox:
    description: "Bounded message queue for actors"

    responsibilities:
      - "Buffer incoming messages"
      - "Priority queue (Normal, High)"
      - "Backpressure signaling"

    guarantees:
      - "FIFO ordering within same priority"
      - "Bounded capacity prevents memory exhaustion"
      - "No message loss in steady state"

    interface:
      - "async fn send(msg: M, priority: Priority) -> Result<()>"
      - "async fn recv() -> Option<Envelope<M>>"
      - "fn capacity() -> usize"

    invariants:
      - name: "FIFO within priority"
        description: "Messages at same priority delivered in send order"

      - name: "Bounded capacity"
        description: "Never exceeds configured capacity"

      - name: "Backpressure"
        description: "Send returns error when full, never blocks indefinitely"
```

### Success Criteria
- ✅ Component defined in system.yaml
- ✅ Guarantees are testable (can write contract tests)
- ✅ Invariants are observable
- ✅ Interface is concrete (types specified)

---

## Phase 2: Red (Write Failing Tests)

**Goal**: Write contract tests that define expected behavior

### Approach

**Manual** - Write tests yourself:
```rust
// tests/contract/mailbox.rs

#[tokio::test]
async fn mailbox_fifo_ordering() {
    let (tx, mut rx) = mailbox(capacity: 3);

    tx.send("msg1", Priority::Normal).await.unwrap();
    tx.send("msg2", Priority::Normal).await.unwrap();
    tx.send("msg3", Priority::Normal).await.unwrap();

    assert_eq!(rx.recv().await, Some("msg1"));
    assert_eq!(rx.recv().await, Some("msg2"));
    assert_eq!(rx.recv().await, Some("msg3"));
}

#[tokio::test]
async fn mailbox_priority_ordering() {
    let (tx, mut rx) = mailbox(capacity: 3);

    tx.send("normal", Priority::Normal).await.unwrap();
    tx.send("high", Priority::High).await.unwrap();
    tx.send("normal2", Priority::Normal).await.unwrap();

    // High priority jumps queue
    assert_eq!(rx.recv().await, Some("high"));
    assert_eq!(rx.recv().await, Some("normal"));
    assert_eq!(rx.recv().await, Some("normal2"));
}

#[tokio::test]
async fn mailbox_backpressure() {
    let (tx, _rx) = mailbox(capacity: 2);

    tx.send("msg1", Priority::Normal).await.unwrap();
    tx.send("msg2", Priority::Normal).await.unwrap();

    // Third send should fail (mailbox full)
    let result = tx.send("msg3", Priority::Normal).await;
    assert!(result.is_err());
}
```

### Run Tests (Expect Failures)
```bash
cargo test --test contract_mailbox
# All tests should FAIL (code doesn't exist yet)
```

### Success Criteria
- ✅ Contract tests written for all guarantees from system.yaml
- ✅ Tests are independent (no shared state)
- ✅ All tests FAIL (red phase)
- ✅ Test names clearly describe behavior

---

## Phase 3: Green (Minimal Implementation)

**Goal**: Write simplest code to make tests pass

### Approach

**Manual** - Implement yourself:
```rust
// crates/mycelium-core/src/mailbox.rs

use tokio::sync::mpsc;
use std::collections::BinaryHeap;

pub struct Mailbox<M> {
    rx: mpsc::Receiver<Envelope<M>>,
    tx: mpsc::Sender<Envelope<M>>,
}

struct Envelope<M> {
    message: M,
    priority: Priority,
    sequence: u64,  // For FIFO within priority
}

impl<M> Mailbox<M> {
    pub fn new(capacity: usize) -> (MailboxSender<M>, MailboxReceiver<M>) {
        let (tx, rx) = mpsc::channel(capacity);
        let sender = MailboxSender { tx, sequence: AtomicU64::new(0) };
        let receiver = MailboxReceiver { rx };
        (sender, receiver)
    }
}

impl<M> MailboxSender<M> {
    pub async fn send(&self, msg: M, priority: Priority) -> Result<(), SendError> {
        let envelope = Envelope {
            message: msg,
            priority,
            sequence: self.sequence.fetch_add(1, Ordering::SeqCst),
        };

        self.tx.send(envelope).await
            .map_err(|_| SendError::MailboxFull)
    }
}
```

### Run Tests (Expect Pass)
```bash
cargo test --test contract_mailbox
# All tests should PASS (green phase)
```

### Success Criteria
- ✅ All contract tests passing
- ✅ Implementation is minimal (no premature optimization)
- ✅ Code compiles without warnings
- ✅ No clippy warnings

---

## Phase 4: Review (Security & Quality)

**Goal**: Automated code review for bugs, security, performance

### Agent Involved

**code-review** (Mandatory after significant implementation)

**When to invoke**:
- After Green phase (tests passing)
- Before Refactor phase
- Significant code written (>50 LOC)

**How to invoke**:
```
Use Task tool with subagent_type: "code-review"

Provide:
- Files: crates/mycelium-core/src/mailbox.rs
- Line ranges: 1-150
- Task file: tasks/implement-mailbox.md
```

**What it reviews**:
- Security vulnerabilities (overflow, injection, etc.)
- Performance issues (unnecessary allocations, locks)
- Correctness bugs (race conditions, edge cases)
- Consistency with project patterns
- Test coverage gaps

**Example prompt**:
```
Review the mailbox implementation for:
- Thread safety issues
- Potential panics or unwraps
- Memory leaks
- Performance bottlenecks
- Missing error handling
```

### Success Criteria
- ✅ No critical security issues
- ✅ No obvious bugs identified
- ✅ Performance acceptable (no red flags)
- ✅ Patterns consistent with existing code

### Action on Findings

**Critical issues**: Fix before proceeding (back to Red phase)
**Minor issues**: Note for Refactor phase
**Performance**: Benchmark before optimizing

---

## Phase 5: Refactor (Improve with Safety)

**Goal**: Improve code quality while keeping tests green

### Approach

**Manual** - Refactor yourself:
- Extract helper functions
- Improve naming
- Add comments/documentation
- Optimize hot paths (if benchmarks justify)

### Rules
1. **Tests stay green** - Run tests after each refactor
2. **One change at a time** - Commit frequently
3. **Preserve behavior** - No new features during refactor

### Run Tests (Continuously)
```bash
# After each refactor
cargo test --test contract_mailbox
```

### Success Criteria
- ✅ All tests still passing
- ✅ Code is more readable
- ✅ Clippy happy (`cargo clippy`)
- ✅ Rustfmt applied (`cargo fmt`)

---

## Phase 6: Document (API + Architecture)

**Goal**: Update documentation to reflect implementation

### Agent Involved

**service-documentation** (for CLAUDE.md updates)

**When to invoke**:
- After implementation complete
- Tests passing
- Code reviewed and refactored

**What it updates**:
- Module-level CLAUDE.md
- API documentation (rustdoc)
- Architecture diagrams (if needed)

**Example**:
```
Use Task tool with subagent_type: "service-documentation"

"Update crates/mycelium-core/CLAUDE.md to document:
- Mailbox API
- Priority queue semantics
- Backpressure behavior
- Usage examples"
```

### Manual Documentation

**Rustdoc**:
```rust
/// Bounded message queue with priority support.
///
/// # Guarantees
/// - FIFO ordering within same priority
/// - Backpressure when capacity reached
/// - No message loss in steady state
///
/// # Example
/// ```
/// let (tx, rx) = Mailbox::new(10);
/// tx.send("msg", Priority::High).await?;
/// let msg = rx.recv().await;
/// ```
pub struct Mailbox<M> {
    // ...
}
```

### Success Criteria
- ✅ Public APIs documented
- ✅ Examples provided
- ✅ CLAUDE.md updated
- ✅ `cargo doc` builds without warnings

---

## Phase 7: Commit (Capture Work)

**Goal**: Clean git history with proper work log

### Agent Involved

**logging** (during context compaction or task completion)

**When to invoke**:
- Task completion
- Context window full
- Sprint end

**What it does**:
- Consolidates work logs
- Updates task file's "Work Log" section
- Organizes by date/session

### Git Commit

**Manual**:
```bash
git add crates/mycelium-core/src/mailbox.rs
git add tests/contract/mailbox.rs
git add docs/GUARDRAILS/system.yaml

git commit -m "Implement bounded mailbox with priority queue

- Add Mailbox struct with configurable capacity
- Implement priority queue (High/Normal)
- Add backpressure when full
- All contract tests passing (100% coverage)

Closes: tasks/implement-mailbox.md

🤖 Generated with Claude Code
Co-Authored-By: Claude <noreply@anthropic.com>"
```

### Success Criteria
- ✅ Commit message describes what & why
- ✅ References task file
- ✅ Work log updated
- ✅ No unrelated changes in commit

---

## Agent Decision Matrix

| Phase | Agent | When to Use | Manual Alternative |
|-------|-------|-------------|-------------------|
| **0. Setup** | scrum-leader | Always (task creation) | N/A |
| **0. Context** | context-gathering | Auto (if no manifest) | N/A |
| **1. Spec** | general-purpose | Complex specs (>50 lines) | Simple specs |
| **2. Red** | None | N/A | Always manual |
| **3. Green** | None | N/A | Always manual |
| **4. Review** | code-review | After >50 LOC written | Small changes |
| **5. Refactor** | None | N/A | Always manual |
| **6. Document** | service-documentation | Module-level docs | API-level docs |
| **7. Commit** | logging | End of task/session | Simple commits |

---

## Example: Complete Cycle

### Starting Point
```bash
# User: "Let's implement the actor mailbox"
/scrum-leader "Create task for bounded mailbox with priority queue"
```

**Result**: `tasks/implement-mailbox.md` created with context.

### Phase 1: Spec
```bash
# Manually edit docs/GUARDRAILS/system.yaml
# Add mailbox component with guarantees
```

### Phase 2: Red
```bash
# Write tests/contract/mailbox.rs
cargo test --test contract_mailbox
# FAIL (expected)
```

### Phase 3: Green
```bash
# Implement crates/mycelium-core/src/mailbox.rs
cargo test --test contract_mailbox
# PASS
```

### Phase 4: Review
```
[Invoke code-review agent]
"Review mailbox.rs for security and performance issues"

Result: No critical issues, suggested adding bounds check
```

### Phase 5: Refactor
```bash
# Add bounds check
# Extract helper function
cargo test --test contract_mailbox
# Still PASS
```

### Phase 6: Document
```
[Invoke service-documentation agent]
"Update mycelium-core/CLAUDE.md with mailbox API"
```

### Phase 7: Commit
```bash
git add -A
git commit -m "Implement bounded mailbox..."
```

---

## Best Practices

### DO
- ✅ Use scrum-leader for ALL task creation
- ✅ Write spec before tests (Phase 1)
- ✅ Write tests before code (Phase 2)
- ✅ Invoke code-review after green (Phase 4)
- ✅ Keep tests green during refactor
- ✅ Commit frequently with clear messages

### DON'T
- ❌ Skip context-gathering (let it run)
- ❌ Write code before tests
- ❌ Skip code review for "small changes"
- ❌ Refactor without tests passing
- ❌ Commit without running tests
- ❌ Use agents for simple tasks (manual is faster)

---

## Workflow Shortcuts

### Quick Cycle (Small Change)
```
Manual Spec → Manual Test → Manual Code → Manual Commit
(Skip agents for <20 LOC changes)
```

### Standard Cycle (Feature)
```
scrum-leader → Manual Spec → Manual Test → Manual Code →
code-review → Manual Refactor → Manual Commit
```

### Complex Cycle (System Component)
```
scrum-leader → context-gathering → general-purpose (spec) →
Manual Test → Manual Code → code-review → Manual Refactor →
service-documentation → logging → Manual Commit
```

---

## Integration with GUARDRAILS

### system.yaml Validation (CI)
```bash
# Before merge
python3 docs/GUARDRAILS/generator/validate_system.py

# Checks:
# - All components in system.yaml have tests
# - All tests reference system.yaml guarantees
# - No code communicates in undocumented ways
```

### Contract Test Coverage (CI)
```bash
# Enforce 100% coverage of system.yaml guarantees
cargo test --test contract_*
cargo tarpaulin --out Xml
```

---

## Summary

**The Flow**:
1. **Setup** task (scrum-leader)
2. **Spec** component (system.yaml)
3. **Red** write failing tests
4. **Green** minimal implementation
5. **Review** code quality (code-review)
6. **Refactor** improve safely
7. **Document** APIs (service-documentation)
8. **Commit** with work log (logging)

**The Philosophy**:
- Spec defines reality
- Tests enforce spec
- Code satisfies tests
- Agents ensure quality
- GUARDRAILS prevent drift

**The Result**:
- Clean architecture
- High test coverage
- Consistent code quality
- Traceable decisions
- No spec-code drift
