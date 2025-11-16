2025-11-05 – Python Integration Roadmap
======================================

Context
-------

Bandit and other Mycelium-based adapters need a first-class way to host Python
services alongside Rust actors. The transport layer already supports UNIX/TCP
sockets; what is missing is schema-aware tooling and operational scaffolding so
Python can speak TLVs without manual byte juggling.

Proposed Steps
--------------

1. Document Frame Contract
   - Describe Mycelium publisher/subscriber frames (topic id, flags, payload
     length, payload).
   - Include examples for UNIX socket and TCP transports.

2. Extend Schema Codegen
   - Teach `mycelium-protocol` (or a companion script) to emit Python packing/
     unpacking helpers based on `contracts.yaml`.
   - Ensure generated code matches Rust layout (endianness, padding) and ships
     with unit tests.

3. Optional Rust Shim Service
   - Implement a small `python-bridge` service that spawns Python workers,
     forwards TLVs over sockets/STDIO, and integrates with Mycelium
     supervision, metrics, and shutdown semantics.

4. IPC Transport Wiring
   - Provide sample configs for UNIX sockets (same host) and TCP (cross host).
   - Offer a minimal Python client that connects, subscribes, and publishes.

5. Testing Harness
   - Build an integration test where a Python script echoes TLVs to verify
     round-trip fidelity.
   - Add CI coverage to detect schema drift.

6. Packaging & Docs
   - Create a `python-sdk/` package with packaging metadata, examples, and
     getting-started docs.
   - Update Bandit/Mycelium READMEs with interoperability instructions.

Benefits of the Shim
--------------------

- Reuse Mycelium restart/backoff strategies and structured telemetry.
- Centralise credentials/config in Rust while Python remains stateless.
- Keep Python workers visible in the existing service graph.

Open Questions
--------------

- Should schema codegen live in `mycelium-protocol` proper or a Bandit-specific
  tool?
- What binary format (TLV vs. JSON/MessagePack) do we expose if teams prioritise
  ergonomics over zero-copy performance?

Owners & Next Actions
---------------------

- Initial owner: Bandit/Mycelium shared infra team.
- Next concrete task: implement Python TLV codegen and publish a prototype
  client + bridge.
