# Mycelium Python Bridge Wire Protocol

This document captures the framing rules Python clients must follow when
communicating with the Mycelium MessageBus via Unix or TCP transports.

## TLV Frame Layout

All messages are encoded as TLV (Type-Length-Value) frames using little-endian
integers:

```
┌──────────┬──────────┬──────────────┐
│ Type ID  │ Length   │ Value        │
│ 2 bytes  │ 4 bytes  │ N bytes      │
│ (u16 LE) │ (u32 LE) │ (payload)    │
└──────────┴──────────┴──────────────┘
```

- **Type ID**: `Message::TYPE_ID` from `contracts.yaml` codegen.
- **Length**: Little-endian 32-bit length of the payload section (not including
  the TL header). `Length <= MAX_PAYLOAD_SIZE (4_194_304 bytes)`.
- **Value**: Bytes of the serialized message. For messages generated from
  `contracts.yaml`, this is a fixed-size struct that matches the Rust layout
  (`#[repr(C)]`, little-endian scalars, explicit padding).

Multiple TLV frames are streamed back-to-back. There is no delimiter beyond the
length field.

## Transport Semantics

### Unix Sockets

- Path configured via `MessageBus::bind_unix_endpoint` or `Topology`.
- Stream-oriented: readers must loop until the entire TLV payload is received.
- Recommended buffer sizes align with `BufferSizes` in
  `crates/mycelium-transport/src/shared.rs`.

### TCP Sockets

- Same framing as Unix sockets, delivered over TCP streams.
- For remote clients, enable TLS or host-level ACLs; authentication is outside
  the scope of TLV framing.

## Example Frame (Hex Dump)

Message: `TradeTick { price: 101_500u64, size: 2_500u64 }`, TYPE_ID = 1200.

```
0000: B0 04   # type_id = 1200 (0x04B0)
0002: 10 00 00 00   # length = 16 bytes
0006: 1C 8F 01 00 00 00 00 00   # price (u64 LE)
000E: C4 09 00 00 00 00 00 00   # size  (u64 LE)
```

## Handshake

Before streaming TLV frames, clients send a schema digest so the bridge can
reject mismatched schemas:

```
┌────────────┬──────────────┐
│ Digest Len │ Digest Bytes │
│ 2 bytes LE │ N bytes      │
└────────────┴──────────────┘
```

- Digest is the SHA-256 of `contracts.yaml` (exposed as `SCHEMA_DIGEST` in both
  Rust and Python bindings).
- Bridges compare the provided digest with their expected value and drop the
  connection on mismatch.
- After a successful handshake, standard TLV frames flow as usual.

## References

- `crates/mycelium-transport/src/codec.rs`
- `docs/implementation/PYTHON_BRIDGE_PLAN.md`
