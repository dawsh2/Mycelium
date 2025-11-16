# Mycelium OCaml SDK (WIP)

This package provides OCaml bindings for Mycelium message contracts plus a
runtime transport that speaks the same TLV protocol used by the Rust
MessageBus.

## Layout

```
ocaml-sdk/
├── dune-project             # Dune workspace definition
├── mycelium.opam            # opam metadata / dependencies
├── lib/
│   ├── runtime.ml           # Primitive encoders/decoders (u16/u64, FixedStr)
│   ├── codec.ml             # TLV framing helpers
│   ├── transport.ml         # Unix/TCP client, publishers/subscribers
│   └── messages.ml          # AUTO-GENERATED from contracts.yaml (see codegen)
└── test/
    └── test_codec.ml        # Alcotest suite (runs via `dune test`)
```

## Generating bindings

```
cargo run -p mycelium-codegen -- \
  --contracts crates/mycelium-protocol/contracts.yaml \
  --ocaml-out ocaml-sdk/lib/messages.ml
```

Re-run the command whenever `contracts.yaml` changes. The generated module
exposes `schema_digest`, one OCaml module per message, and encode/decode helper
functions.

## Native runtime (FFI)

The `Mycelium.Native_runtime` module embeds the Rust MessageBus directly via the
`mycelium-ffi` C ABI. This is useful when you need zero-copy publish/subscribe
without the Unix/TCP bridge.

### Build prerequisites

1. Build the `mycelium-ffi` crate so the shared/static library exists:

   ```sh
   cargo build -p mycelium-ffi --release
   export MYCELIUM_FFI_LIB_DIR="$(pwd)/target/release"
   ```

2. Build the OCaml library:

   ```sh
   cd ocaml-sdk
   dune build
   ```

Dune reads `MYCELIUM_FFI_LIB_DIR` to locate `libmycelium_ffi` (defaults to
`../../target/debug`). Update the variable if you prefer the release build.

### Example

```ocaml
open Mycelium

let runtime = Native_runtime.create ()

let () =
  Native_runtime.subscribe runtime
    ~type_id:Messages.TextMessage.type_id
    ~decode:Messages.TextMessage.decode
    ~callback:(fun msg -> Printf.printf "Rust -> OCaml: %s\n" msg.content)

let outbound = Messages.TextMessage.{ sender = "ocaml"; content = "hi"; timestamp = 42L }

let () =
  Native_runtime.publish runtime
    ~type_id:Messages.TextMessage.type_id
    Messages.TextMessage.encode
    outbound
```

Call `Native_runtime.close` (or rely on GC finalizers) when you are done to
shut down the embedded runtime.

## Next steps

- Expand unit tests (transport, publisher/subscriber, native runtime) under
  `ocaml-sdk/test`.
- Integrate dune builds into CI (opam switch, `dune build`, `dune test`).
- Publish the package to opam once integration tests land.
