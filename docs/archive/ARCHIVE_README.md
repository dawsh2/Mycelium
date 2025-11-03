# Archive: Research and Abandoned Approaches

This directory contains documentation for designs and approaches that were researched but ultimately not pursued for Mycelium.

## Files in This Archive

### `actors.md` - Full Actor Framework Design

**Status**: Not implemented  
**Date archived**: 2025-11-02  
**Reason**: Architectural pivot to simpler pub/sub transport layer

**What it was**: 
Comprehensive design document for an Erlang-style actor framework with:
- Actor supervision trees
- Point-to-point mailboxes
- Actor lifecycle management (pre_start, handle, post_stop)
- Actor registry and discovery
- RPC-style call patterns

**Why we didn't build it**:
1. **Overcomplicated for our use case** - We have linear data pipelines (blockchain → strategy → executor), not complex actor graphs
2. **Supervision not needed** - systemd and Kubernetes handle process restarts better than custom supervision
3. **Pub/sub is simpler** - One-to-many event streaming matches our architecture better than one-to-one actor messages
4. **Development time** - Full actor framework would take 12 weeks; transport layer took 2 weeks

**What we built instead**: 
Simple pub/sub messaging with adaptive transport (Arc → Unix → TCP). See `TRANSPORT.md` in the root directory.

**Value of this document**:
- Shows depth of research done before making architectural decision
- Useful reference if we ever need actor patterns in the future
- Documents the "why not" for future team members

---

## Related Research

See also:
- `docs/research/ACTOR_FRAMEWORK_COMPARISON.md` - Evaluation of existing Rust actor frameworks
- `docs/research/RACTOR_FORK_ANALYSIS.md` - Analysis of Ractor framework internals
- `docs/research/CUSTOM_ACTOR_IMPLEMENTATION_GUIDE.md` - How we would build custom actors
- `docs/research/ACTOR_FRAMEWORK_DECISION_SUMMARY.md` - Final decision rationale

**Key insight from research**: Building custom actor framework would require invasive changes to support adaptive transport (Arc → Unix → TCP). Simpler to build transport layer without full actor semantics.

---

## Lessons Learned

1. **Research before committing** - We spent time understanding existing frameworks before deciding to build custom
2. **Simplicity wins** - Cutting scope from "full actor framework" to "pub/sub transport" saved 10 weeks
3. **Match architecture to problem** - Linear data pipelines → pub/sub, not actor graphs
4. **Document decisions** - This archive explains why we chose what we did

---

## When to Revisit

Consider actor patterns if we encounter:
- Complex stateful services that need lifecycle management
- Need for fine-grained failure isolation (beyond process boundaries)
- Request/reply patterns becoming common
- Dynamic actor spawning during runtime

For now, pub/sub + systemd/k8s is sufficient.
