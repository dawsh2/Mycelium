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
from mycelium_protocol import SCHEMA_DIGEST, TextMessage

transport = UnixTransport("/tmp/mycelium/python.sock", SCHEMA_DIGEST)
transport.connect()

publisher = transport.publisher(MyMessage)
subscriber = transport.subscriber(MyMessage)

publisher.publish(TextMessage(sender="bot", content="ping", timestamp=123))
msg = subscriber.recv(timeout=1.0)
print(msg)

# When running under the bridge service, these environment variables are
# exported automatically:
#   MYCELIUM_SOCKET=/var/run/mycelium/python.sock
#   MYCELIUM_SCHEMA_DIGEST=<hex string>
```

Every connection performs a handshake using `SCHEMA_DIGEST` so mismatched
schemas are rejected by the Rust bridge.

See `tests/fixtures/python_bridge_echo.py` for a complete worker example that
publishes/consumes `TextMessage` frames and writes its observations to a JSON
file. The integration test (`cargo test python_bridge_service`) uses that script
to verify the full Rust ↔ Python round trip.
