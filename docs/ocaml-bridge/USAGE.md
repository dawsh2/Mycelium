# Mycelium OCaml Bridge & SDK Guide

This guide walks through generating OCaml bindings from `contracts.yaml`,
building the OCaml SDK, and running the `OcamlBridgeService` integration tests.

## 1. Prerequisites

- OCaml 4.14+ (opam) with `dune >= 3.10` on your `PATH`.
- OCaml packages: `ocplib-endian`, `yojson`, `alcotest`.
- Rust toolchain (`cargo`, `rustup`).

## 2. Generate OCaml message bindings

```sh
cargo run -p mycelium-codegen -- \
  --contracts crates/mycelium-protocol/contracts.yaml \
  --ocaml-out ocaml-sdk/lib/messages.ml
```

Re-run when `contracts.yaml` changes. The emitted module exposes `schema_digest`
and per-message modules (`TextMessage`, etc.) with `type t`, `encode`, and
`decode` helpers.

## 3. Build and test the OCaml SDK

```sh
cd ocaml-sdk
opam install . --deps-only --with-test
dune build
dune test
```

Artifacts land under `_build/`. The integration tests expect the worker fixture
at `_build/default/test/ocaml_bridge_echo.exe` (the `.exe` suffix may be absent
on Unix).

## 4. Run Rust ↔ OCaml integration tests

```sh
dune build test/ocaml_bridge_echo.exe   # optional; cargo test builds if missing
cargo test ocaml_bridge_service -- --nocapture
```

The suite performs:

1. Manual TLV loopback (no child) to validate the handshake and TLV forwarding.
2. OCaml worker round trip, which builds the OCaml echo fixture (skipping with a
   warning if `dune`/artifact is missing), launches it via `OcamlBridgeService`,
   and asserts both directions of `TextMessage` delivery.

## 5. Writing an OCaml worker

```ocaml
let digest = Mycelium.Messages.schema_digest in
let socket = Sys.getenv "MYCELIUM_SOCKET" in
let transport =
  Mycelium.Transport.connect_unix ~socket_path:socket ~schema_digest:digest
in
let pub = Mycelium.Transport.publisher transport ~type_id:TextMessage.type_id in
let sub = Mycelium.Transport.subscribe transport ~type_id:TextMessage.type_id in
let outbound = { TextMessage.sender = "ocaml"; content = "hi";
                 timestamp = Int64.of_float (Unix.time ()) } in
Mycelium.Transport.send pub (TextMessage.encode outbound);
let inbound = Mycelium.Transport.recv sub |> TextMessage.decode in
(* handle inbound *)
```

Bridge-provided env vars:

- `MYCELIUM_SOCKET` – Unix socket path to connect to.
- `MYCELIUM_SCHEMA_DIGEST` – schema digest hex string (optional helper in SDK to
  convert to bytes).
- Additional vars (e.g., `MYCELIUM_TEST_OUTPUT`) can be set via
  `OcamlChildConfig`.

## 6. Troubleshooting

- **Digest mismatch:** Regenerate `ocaml-sdk/lib/messages.ml` so the digest
  matches the Rust side; the bridge will otherwise reject the handshake.
- **`dune` missing:** The integration test logs and skips the OCaml worker test
  if it cannot run `dune build` or locate the executable.
- **No messages:** Ensure the OCaml worker subscribes to the correct `type_id`
  and that the bridge socket path matches the env var.

Refer to `docs/implementation/OCAML_BRIDGE_PLAN.md` for architecture details.
