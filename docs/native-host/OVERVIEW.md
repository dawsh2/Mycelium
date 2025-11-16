# Native Host Plan (Python & OCaml)

## Goals

- Provide an optional "native" integration path so Python or OCaml code can run inside the same process as Rust services without using socket bridges.
- Expose a tiny C ABI for publish/subscribe so foreign runtimes can attach directly to the `MessageBus` while reusing existing TLV encoders, schema digests, and metrics.
- Ship first-class adapters for Python (PyO3) and OCaml (dune/C stubs) with developer-friendly APIs (`runtime.publish(msg)` / `runtime.subscribe(msg, cb)`).

## Architecture

```
+--------------------+       +--------------------------+
|  Foreign runtime   | <----> |  mycelium_native adapter |
| (Python / OCaml)   |       +-----------+--------------+
+---------+----------+                   |
          | C ABI                        | Rust FFI shim
          v                              v
    +-----+-------------------------------+------+
    |        mycelium-ffi (Arc<MessageBus>)     |
    +-------------------+-----------------------+
                        |
                +-------+--------+
                | MessageBus /  |
                | LocalTransport|
                +---------------+
```

- **C ABI**: `mycelium_runtime_create/destroy`, `verify_schema_digest`, `publish`, `subscribe`, `unsubscribe`.
- **Rust shim**: Owns a Tokio runtime, forwards commands to `MessageBus`, manages callback fanout, catches panics, and enforces schema digests.
- **Adapters**:
  - *Python*: PyO3 module (`mycelium_native.Runtime`) that calls the C ABI, acquires the GIL for callbacks, and reuses generated dataclasses for encode/decode.
  - *OCaml*: dune project with C stubs; callbacks are GC-rooted and invoked via `caml_callback`.

## Milestones

1. **FFI crate (`mycelium-ffi`)**
   - Set up crate + `cbindgen` header.
   - Implement runtime handle, publish/subscribe, schema check, panic guards.
   - Unit tests via `libloading`.

2. **Python adapter**
   - PyO3 + maturin project.
   - `Runtime` class wraps init/destroy, uses generated message bindings for ergonomics.
   - pytest + integration tests.

3. **OCaml adapter**
   - Extend `ocaml-sdk` with C stubs and `runtime.ml` wrapper.
   - Manage GC roots, runtime lock, and expose nice API.
   - dune tests + integration harness.

4. **Integration tests & CI**
   - Rust tests embedding Python/OCaml runtimes in-process.
   - CI job installing OCaml/dune + maturin to run new suites.

5. **Docs & rollout**
   - Usage guides per language, troubleshooting, sample code.
   - README mentions native mode; existing bridge docs link to native host option.

## Risks & Mitigations

- **Panic/exception across FFI** → wrap entrypoints with `catch_unwind`, convert to error codes.
- **GC safety** → register callback roots (OCaml), hold Py objects while runtime alive, document lifetime rules.
- **Thread safety** → callbacks dispatched on dedicated executor; users re-enter their runtimes via provided hooks.
- **ABI stability** → version header, keep surface minimal, document breaking changes.

## Testing Strategy

- Rust unit/integration tests for `mycelium-ffi` (publish/subscribe, digest mismatch, shutdown).
- Python pytest suite + Rust embed test.
- OCaml dune tests + Rust embed test.
- Sanitizers (ASAN/TSAN) in CI for FFI crate; optional valgrind job for leak detection.

## Developer Experience

- Provide helper APIs that accept generated message types, hiding type IDs + encoding details.
- Offer both synchronous callbacks and opt-in async adapters (asyncio/Lwt) in later iterations.
- Supply sample projects demonstrating how to embed the native runtime in a Rust binary.
