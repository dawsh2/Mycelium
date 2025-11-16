# Python Bridge Usage Guide

This document explains how to generate Python bindings from `contracts.yaml`,
use the topology-aware MessageBus, and integrate Python services with Mycelium.

## Quick Start (Topology-Aware API)

**New in v0.2.0**: Python services can now use topology files for automatic routing!

```python
from mycelium import MessageBus
from mycelium_protocol import SCHEMA_DIGEST
from mycelium_protocol.messages import TradeSignal

# Load topology and auto-configure transports
bus = MessageBus.from_topology(
    "topology.toml",
    service_name="python-worker",
    schema_digest=SCHEMA_DIGEST
)

# Auto-routes based on topology (Unix or TCP)
pub = bus.publisher_to("strategy-service", TradeSignal)
pub.publish(TradeSignal(symbol="BTC", action="buy"))

# Subscribe locally
sub = bus.subscriber(TradeSignal)
for msg in sub:
    print(f"Received: {msg}")
```

See [Cross-Language Integration](../architecture/CROSS_LANGUAGE_INTEGRATION.md) for architecture details and deployment patterns.

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

## 3. Observability & Handshake

- Every generated Python transport requires the 32-byte `SCHEMA_DIGEST`. Import
  it from `mycelium_protocol` and pass it to `UnixTransport`/`TcpTransport` so
  the bridge can reject incompatible clients before streaming TLVs.
- The Rust bridge now records connection counts, handshake failures, and
  forwarded-frame totals using `ServiceMetrics`. These counters show up in the
  service log every few seconds and in the shutdown summary, so you can plug the
  bridge into the same monitoring dashboards as native Rust actors.

## 4. Integration Test Harness

A cross-language test lives in `tests/transport_tests/python_bridge_service.rs`.
It launches the bridge service inside `ServiceRuntime`, spawns a supervised
Python worker (see `tests/fixtures/python_bridge_echo.py`), and asserts that:

1. Python publishes a `TextMessage` that Rust subscribers observe, and
2. Rust publishes a `TextMessage` that the Python worker captures and records.

This test runs via `cargo test python_bridge_service` and doubles as a sample of
the environment variables (`MYCELIUM_SOCKET`, `MYCELIUM_SCHEMA_DIGEST`,
`MYCELIUM_TEST_OUTPUT`) that supervised workers rely on.

## 5. Next Steps

With bindings in place we can flesh out additional transports (TCP), integrate
Python metrics into cluster dashboards, and follow the roadmap in
`docs/implementation/PYTHON_BRIDGE_PLAN.md`. See also
`docs/ocaml-bridge/USAGE.md` for the OCaml bridge, which reuses the same wire
protocol and supervision infrastructure.
