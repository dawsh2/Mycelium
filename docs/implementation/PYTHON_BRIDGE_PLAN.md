# Python Bridge Implementation Plan

## Goals

- Enable Python services (data adapters, strategies, risk, execution) to publish
  and subscribe to Mycelium TLV messages.
- Keep the schema-first workflow (`contracts.yaml`) and zero-copy guarantees on
  the Rust side while generating safe Python bindings.
- Give Python workers first-class observability and supervision so they appear
  in the same topology/metrics views as Rust services.

## Workstreams

### 1. Wire Protocol & Schema Codegen

1. Document TLV framing (type_id, payload_len, payload) with Unix/TCP examples
   in `docs/python-bridge/wire-protocol.md`.
2. Extend `mycelium-protocol` codegen to emit Python pack/unpack helpers that
   mirror Rust layouts (endianness, padding, validation). Add a CLI flag
   (`mycelium-codegen --python`).
3. Embed schema digests/handshakes so bridges and clients can detect drift at
   connect time.

### 2. Python SDK

1. Package transports (`UnixTransport`, `TcpTransport`), TLV codec, and
   `Publisher`/`Subscriber` APIs with asyncio integration.
2. Ship the generated schema module (`mycelium_protocol.messages`) alongside
   runtime utilities.
3. Provide metrics/tracing hooks plus docs/examples showing how to publish and
   subscribe from Python services.

### 3. Rust Bridge Service

1. Implement a `mycelium-python-bridge` actor (supervised by `ServiceRuntime`)
   that proxies TLVs between the MessageBus and Python sockets; optionally spawn
   and monitor Python subprocesses.
2. Expose config (socket path, ACLs, buffer sizes) via topology files so
   bridges deploy like any other service.
3. Surface metrics/logs (publish counts, errors, worker status) via existing
   observability plumbing.

### 4. Testing & CI

1. Add round-trip tests (Rust ↔ Python echoes, validation failures,
   reconnection scenarios).
2. Write performance tests for Unix/TCP latency and throughput.
3. Add a CI job that regenerates Python code, runs SDK tests (pytest), and
   fails on schema drift.

### 5. Documentation & Deployment

1. Author usage guides (wire protocol, SDK cookbook, topology examples).
2. Provide packaging instructions (PyPI wheel for the SDK, Dockerfile for the
   bridge).
3. Document how this pattern generalizes to other languages (e.g., Julia) so
   future ports reuse the same pipeline.

