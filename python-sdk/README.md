# Mycelium Python SDK (WIP)

Scaffolding for the upcoming Python client and bridge integration. Final APIs
will enable Python services to publish and subscribe TLV-framed Mycelium
messages over Unix/TCP transports.

## Generating Bindings

Run the workspace CLI to regenerate schema bindings:

```
cargo run -p mycelium-codegen -- \\
  --contracts contracts.yaml \\
  --python-out python-sdk/mycelium_protocol/messages.py
```

This ensures the Python dataclasses stay in sync with the Rust types generated
in `crates/mycelium-protocol`.

## Connecting to the Bridge

```python
from mycelium import UnixTransport
from mycelium_protocol import SCHEMA_DIGEST

transport = UnixTransport("/tmp/mycelium/python.sock", SCHEMA_DIGEST)
transport.connect()

publisher = transport.publisher(MyMessage)
subscriber = transport.subscriber(MyMessage)
```

Every connection performs a handshake using `SCHEMA_DIGEST` so mismatched
schemas are rejected by the Rust bridge.
