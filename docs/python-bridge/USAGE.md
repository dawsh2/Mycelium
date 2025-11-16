# Python Bridge Usage Guide

This document explains how to generate Python bindings from `contracts.yaml`
and where the SDK pieces live.

## 1. Generate Bindings

Use the new `mycelium-codegen` CLI (workspace crate) to emit Rust and Python
artifacts from the same schema:

```
cargo run -p mycelium-codegen -- \
  --contracts contracts.yaml \
  --rust-out crates/mycelium-protocol/src/generated.rs \
  --python-out python-sdk/mycelium_protocol/messages.py
```

Options:

- `--contracts`: path to your schema file (defaults to `contracts.yaml`).
- `--rust-out`: optional; regenerate the Rust bindings.
- `--python-out`: optional; generate the Python module.
- `--external-imports`: use external paths when generating Rust for downstream
  crates.

CI should run the CLI and fail if either output differs, ensuring schema drift
is detected early.

## 2. Python SDK Layout

- `python-sdk/mycelium/` – transport/runtime code (work in progress).
- `python-sdk/mycelium_protocol/messages.py` – generated dataclasses.
- `python-sdk/mycelium/protocol/runtime.py` – shared encoder/decoder helpers
  that the generated module imports.

## 3. Next Steps

With bindings in place we can flesh out the transport client (Unix/TCP), the
Rust bridge service, and observability hooks described in
`docs/implementation/PYTHON_BRIDGE_PLAN.md`.
