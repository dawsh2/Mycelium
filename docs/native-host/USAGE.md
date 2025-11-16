# Native Host Usage Guide

## 1. Build artifacts

- `libmycelium_ffi.{a,so}` (C ABI)
- `mycelium.h` header
- Python adapter wheel (`mycelium_native`)
- OCaml adapter dune/opam package (`mycelium_native`)

## 2. Runtime lifecycle

1. Create runtime (`mycelium_runtime_create` / `Runtime()` wrappers)
2. Verify schema digest (`mycelium_verify_schema_digest`)
3. Publish messages via generated encoders → `mycelium_publish`
4. Subscribe with callbacks per `type_id`
5. Destroy runtime to shut down Tokio executor

## 3. Python sketch

    from mycelium_native import Runtime
    from mycelium_protocol import TextMessage, SCHEMA_DIGEST

    rt = Runtime(SCHEMA_DIGEST)

    def handle(msg: TextMessage):
        print("Rust -> Python", msg)

    rt.subscribe(TextMessage, handle)
    outbound = TextMessage(sender="py", content="hi", timestamp=123)
    rt.publish(outbound)

### Building the native module

```
cd crates/mycelium-python-native
# optional but recommended: install a shared CPython via pyenv first
#   PYTHON_CONFIGURE_OPTS="--enable-shared" pyenv install 3.11.10
PYENV_PY=$HOME/.pyenv/versions/3.11.10
PYO3_PYTHON=$PYENV_PY/bin/python3.11 \
RUSTFLAGS="-C link-arg=-L${PYENV_PY}/lib -C link-arg=-lpython3.11 -C link-arg=-ldl -C link-arg=-framework -C link-arg=CoreFoundation -C link-arg=-Wl,-rpath,${PYENV_PY}/lib" \
maturin develop --release
```

This produces the `mycelium_native` extension module in the current virtualenv. It is built
against the stable CPython abi3 surface (≥3.9), so the same wheel works for Python 3.9–3.12.

### Runtime lifecycle helpers

- `Runtime()` defaults to the baked-in `SCHEMA_DIGEST`, or accept `Runtime(schema_digest=b"...")`
  to override.
- `Runtime.publish(message)` pulls `TYPE_ID`/`to_bytes()` from the generated dataclass and hands the
  payload to the Rust bus via `mycelium_publish`.
- `Runtime.subscribe(MessageCls, callback)` decodes bytes via `MessageCls.from_bytes` and calls the
  provided Python callable on a background Tokio task. The method returns a `Subscription` with a
  `close()` method so callers can tear down callbacks explicitly.
- All callbacks reacquire the GIL internally, so user code can interact with asyncio/event loops as
  needed. Exceptions are logged via `tracing` + `PyErr::print` but do not crash the runtime.

## 4. OCaml sketch

    open Mycelium_native

    let runtime = Runtime.create My_messages.schema_digest

    let on_msg (msg : My_messages.TextMessage.t) =
      Printf.printf "Rust -> OCaml: %s\n" msg.content

    let () =
      Runtime.subscribe runtime My_messages.TextMessage.t on_msg;
      let outbound = My_messages.TextMessage.{ sender = "ocaml"; content = "hello"; timestamp = 123L } in
      Runtime.publish runtime outbound

## 5. Troubleshooting

- Schema mismatch → regenerate bindings so `SCHEMA_DIGEST` matches
- Callback errors → wrap user functions, log failures
- Threading → Python callbacks reacquire the GIL; OCaml callbacks run with runtime lock

## 6. Native vs. bridge

- Choose native mode when embedding runtimes is acceptable and latency-critical
- Stick with Unix/TCP bridges for process isolation and independent deployments

## 7. Packaging checklist

- `crates/mycelium-python-native/pyproject.toml` wires up `maturin` for wheel builds
- `cargo build -p mycelium-python-native --release` emits `libmycelium_native.{so,dylib}` for
  embedding scenarios outside of Python packaging
- `cbindgen` can export headers from `mycelium-ffi` when other languages need to consume the same
  C ABI directly
- `maturin build --release` (with the same env vars above) emits a universal wheel at
  `target/wheels/mycelium_native-<version>-cp39-abi3-macosx_11_0_arm64.whl`
- `cargo test -p mycelium-python-native` also needs those env vars so the linker/dyld can find
  `libpython3.11.dylib`; consider adding them to your shell profile or `.envrc`
