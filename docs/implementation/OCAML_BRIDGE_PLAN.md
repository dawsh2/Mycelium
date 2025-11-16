# OCaml Bridge Implementation Plan

> Drafted: November 16, 2025

This document extends the Python bridge work to support OCaml clients that need
first-class access to the Mycelium MessageBus. The intent is to reuse the
existing TLV wire protocol, schema digest handshake, and supervision patterns
while delivering an idiomatic OCaml SDK and integration tests.

---

## Goals

1. **Parity with the Python bridge** – OCaml services should connect via Unix or
   TCP sockets, perform the schema digest handshake, and benefit from the same
   routing, backpressure, metrics, and restart semantics as Rust actors.
2. **Generated bindings** – All types defined in `contracts.yaml` must have
   OCaml representations plus encode/decode helpers that are byte-compatible
   with the Rust zerocopy layouts.
3. **Developer ergonomics** – Ship an `ocaml-sdk/` (dune/opam project) with
   transports, publishers/subscribers, and starter workers so teams can scaffold
   services quickly.
4. **Safe supervision** – The Rust runtime should be able to spawn and monitor
   OCaml workers just like the existing `PythonBridgeService`, emitting the same
   metrics and logs.
5. **CI coverage** – Add cross-language tests that prove Rust ↔ OCaml
   round-trips, schema enforcement, and graceful shutdown.

### Non-goals

- Compile-time routing and zero-copy `Arc<T>` sharing: only Rust services
  running in-process get these features; external languages will continue to use
  IPC/TCP transports.
- Tight OCaml↔Rust FFI embedding: communication will occur over sockets, not via
  OCaml stubs linked into the Rust binary (at least in this phase).

---

## Architecture Overview

### Reused Components

- **Socket Endpoints** (`crates/mycelium-transport/src/socket_endpoint.rs`):
  already implement TLV framing, fanout, schema digest verification, and shared
  metrics (`EndpointStats`).
- **ServiceRuntime** (`crates/mycelium-transport/src/service_runtime.rs`):
  provides supervision, restarts, and `ServiceMetrics` integration.
- **Wire Protocol Docs** (`docs/python-bridge/wire-protocol.md`): reuse TLV
  framing references; only language-specific examples will change.

### New Components

| Layer | Deliverable | Notes |
| --- | --- | --- |
| Codegen | `generate_ocaml_from_yaml` | Deserializes `contracts.yaml`, emits OCaml modules with digest constants and encode/decode functions. |
| SDK | `ocaml-sdk/` | Contains runtime primitives (`runtime.ml`), TLV codec, Unix/TCP transports, publishers/subscribers, dune/opam metadata, README/tests. |
| Bridge | `ocaml_bridge.rs` (or generalized foreign bridge) | Binds sockets, enforces digests, optionally spawns OCaml child workers, logs stats via `EndpointStats`. |
| Fixtures | `tests/fixtures/ocaml_bridge_echo.ml` | Example worker compiled via dune for integration tests. |
| Tests | `tests/transport_tests/ocaml_bridge_service.rs` | Mirrors Python tests: manual TLV handshake + supervised worker round trip. |
| Docs | `docs/ocaml-bridge/USAGE.md` | Install/run instructions, troubleshooting, observability guidance. |

### Generalize vs. Duplicate Bridge Logic

We have two options:

1. **Generalized foreign bridge** – Extract the common pieces of
   `PythonBridgeService` (socket binding, stats, child supervision, env handling)
   into a reusable module (e.g., `ForeignBridgeService`). Each language supplies
   a configuration struct for child process args/env overrides. Pros: one code
   path for metrics and lifecycle; easier to add future languages. Cons: may
   over-generalize too early and complicate language-specific knobs.
2. **Dedicated `ocaml_bridge.rs`** – Start with a copy of `python_bridge.rs` and
   modify where necessary. Pros: faster to get running; can diverge if OCaml has
   unique needs (e.g., dune workflows). Cons: duplicated maintenance.

**Recommendation:** Begin with a light abstraction—a shared helper for binding
socket endpoints and launching child processes—but keep thin language-specific
wrappers so each SDK can evolve independently. Revisit full generalization once
two bridges are stable.

---

## Milestones & Tasks

### M1. Architecture Blueprint & Tooling Decisions (Week 1)
- Draft this document (complete when merged) plus an ADR capturing the bridge
  generalization decision and OCaml tooling stack (opam, dune, ocplib-endian).
- Spike on OCaml encode/decode strategy: verify we can reproduce the TLV byte
  layout using `Bytes` + explicit offsets.
- Deliverable: merged doc, ADR, spike notes.

### M2. Schema Codegen & Runtime Primitives (Week 1-2)
- Update `crates/mycelium-codegen/src/main.rs` to accept `--ocaml-out` and call a
  new `generate_ocaml_from_yaml` helper.
- Extend `crates/mycelium-protocol/src/codegen.rs` with OCaml-specific emitters:
  type mapping helpers, encode/decode generation, schema digest constant.
- Add `ocaml-sdk/lib/runtime.{ml,mli}` containing primitive encoders for
  integers, fixed strings (with 6-byte padding), and fixed byte arrays.
- Tests:
  - Rust unit tests comparing generated OCaml source against golden fixtures.
  - Cross-language golden test: encode a `TextMessage` in Rust, decode with the
    generated OCaml code (executed via dune test) and vice versa.

### M3. OCaml SDK Transports (Week 2-3)
- Scaffold `ocaml-sdk/` project (dune-project, opam file, README).
- Implement TLV codec (`lib/codec.ml`) and Unix/TCP transports (`lib/transport.ml`).
- Provide publisher/subscriber modules with blocking queues and logging, plus a
  future-facing plan for Lwt/Async wrappers.
- Unit tests via `dune test` (Alcotest) for framing, handshake, and queue logic.

### M4. OCaml Bridge Service in Rust (Week 3-4)
- Introduce `OcamlBridgeConfig`, `OcamlChildConfig`, and `OcamlBridgeService`
  (new file or refactored generic bridge). Ensure it:
  - Binds Unix socket endpoints via `MessageBus::bind_unix_endpoint_with_digest`.
  - Spawns OCaml workers (compiled dune executable) when configured, wiring
    `MYCELIUM_SOCKET`, `MYCELIUM_SCHEMA_DIGEST`, and optional env overrides.
  - Shares `EndpointStats` with ServiceMetrics for observability.
  - Shuts down gracefully (child kill, socket teardown, stats log).
- Update `crates/mycelium-transport/src/lib.rs` exports and add an example
  (`examples/ocaml_bridge_demo.rs`).

### M5. Integration Tests & CI (Week 4-5)
- Implement `tests/fixtures/ocaml_bridge_echo.ml` that publishes/subscribes to
  `TextMessage` and dumps a JSON summary.
- Add dune rules to build the fixture; ensure artifacts land under `target/` so
  Rust tests can execute them.
- Add `tests/transport_tests/ocaml_bridge_service.rs` (manual TLV + supervised
  worker tests) and update `tests/transport_tests/mod.rs`.
- Extend CI workflow to install OCaml (via `ocaml/setup-ocaml`), run dune tests,
  build the fixture, and execute `cargo test ocaml_bridge_service` alongside the
  existing Python suite.

### M6. Packaging, Docs, Observability (Week 5-6)
- Publish `ocaml-sdk` as an opam package (versioned alongside the workspace).
- Author `docs/ocaml-bridge/USAGE.md`, update the main README, and add
  deployment guidance (Dockerfile/systemd template for OCaml workers).
- Ensure dashboards include OCaml bridge metrics; document the log fields and
  alert thresholds.

---

## Testing Strategy

| Layer | Test | Notes |
| --- | --- | --- |
| Codegen | Rust unit tests comparing generated OCaml files | Ensures deterministic output. |
| SDK | `dune test` (Alcotest) for codec/transport | Validates encode/decode, handshake, queue semantics. |
| Bridge | `cargo test ocaml_bridge_service` | Two tests: manual TLV client and supervised OCaml worker. |
| Handshake | Negative tests (wrong digest, truncated frames) | Mirror Python coverage. |
| CI | Github Actions job installing OCaml toolchain | Runs dune tests + Rust integration tests. |

---

## Risks & Mitigations

1. **Byte-layout drift** – Mitigate with golden tests comparing Rust ↔ OCaml
   serialization and by reusing the same `SCHEMA_DIGEST` constant.
2. **Toolchain complexity** – Use `ocaml/setup-ocaml` action and cache opam
   switches to keep CI predictable; document local setup (install opam, dune,
   ocamlformat, ocaml-lsp).
3. **Child supervision differences** – Provide reference OCaml worker template
   with signal handlers; ensure `OcamlBridgeService` enforces timeout + kill
   semantics on shutdown.
4. **Performance under GC pauses** – Recommend Lwt-based transport in docs and
   document tuning tips (socket buffer sizes, cooperative scheduling).
5. **Future generalization** – Track whether Python and OCaml bridges diverge; if
   duplication becomes a burden, revisit a true `ForeignBridgeService` once both
   are stable.

---

## Open Questions

1. **Bridge generalization** – How much code should be shared between Python and
   OCaml bridges? (Decision pending prototype.)
2. **Async model** – Should the first SDK release rely on blocking threads or
   ship an Lwt/Async wrapper immediately? (Leaning toward blocking first with a
   roadmap for Lwt adapters.)
3. **Packaging cadence** – Should OCaml SDK releases follow the Rust crate
   version (e.g., 0.1.x) or use independent semantic versioning?
4. **TCP transport requirements** – Do OCaml clients need TLS for remote
   deployments in this phase, or is Unix sockets sufficient?

---

## Next Steps

1. Review and sign off on this plan.
2. Kick off Milestone 2 (codegen + runtime primitives) once the design decisions
   above are approved.
3. Track progress via the existing milestone plan: codegen → SDK → bridge →
   tests → packaging.

