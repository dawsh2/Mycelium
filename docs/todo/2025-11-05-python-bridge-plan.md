# 2025-11-05 Python Bridge Plan

## Goal
Document the work required to allow non-Rust services (e.g., Python strategies) to publish/subscribe over Mycelium's message bus while preserving typed TLV contracts.

## Milestones
1. **Frame Contract Reference**
   - Document the socket frame format (topic, length, payload) emitted by `MessageBus`.
   - Explain back-pressure semantics and ordering guarantees for external clients.

2. **Schema Codegen for Python**
   - Extend `mycelium-protocol` codegen to emit a Python package with pack/unpack helpers for TLVs defined in `contracts.yaml`.
   - Include type tests to ensure the Python serializers round-trip with Rust references.

3. **Bridge Service**
   - Implement `crates/python-bridge` (Rust) that subscribes/publishes selected topics and forwards them over a Unix socket to Python workers.
   - Handle process supervision, restart backoff, and graceful shutdown hooks.

4. **Python SDK Skeleton**
   - Create `python-sdk/` with generated message types, async client, and examples for consuming/publishing TLVs.
   - Ship packaging metadata (pyproject.toml) and CI sanity tests.

5. **Integration Test Harness**
   - Add a cross-language test that spins up the bridge, runs a sample Python worker, and validates TLV echo + schema compatibility.
   - Capture performance baselines (messages/sec) to compare native vs. bridged flows.

6. **Observability & Ops**
   - Emit structured metrics for bridge throughput and failure counters.
   - Document deployment playbook (config flags, env vars, failure modes) in `docs/integration/python-clients.md`.

## Open Questions
- Should the bridge expose a gRPC control plane for Python workers to subscribe/unsubscribe at runtime?
- What security model do we need for cross-language IPC (e.g., socket permissions, TLS for TCP transport)?
