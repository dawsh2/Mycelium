"""TLV framing helpers shared by transports."""

from __future__ import annotations

import socket
import struct
from typing import Tuple

HEADER_SIZE = 6
MAX_PAYLOAD_SIZE = 4_194_304


def encode_frame(type_id: int, payload: bytes) -> bytes:
    if not 0 <= type_id <= 0xFFFF:
        raise ValueError(f"type_id {type_id} out of range")
    payload_len = len(payload)
    if payload_len > MAX_PAYLOAD_SIZE:
        raise ValueError(
            f"payload size {payload_len} exceeds MAX_PAYLOAD_SIZE {MAX_PAYLOAD_SIZE}"
        )
    header = struct.pack("<HI", type_id, payload_len)
    return header + payload


def read_frame(sock: socket.socket) -> Tuple[int, bytes]:
    header = _recv_exact(sock, HEADER_SIZE)
    type_id, payload_len = struct.unpack("<HI", header)
    if payload_len > MAX_PAYLOAD_SIZE:
        raise ValueError(
            f"peer attempted to send {payload_len} bytes (limit {MAX_PAYLOAD_SIZE})"
        )
    payload = _recv_exact(sock, payload_len)
    return type_id, payload


def _recv_exact(sock: socket.socket, length: int) -> bytes:
    buf = bytearray(length)
    view = memoryview(buf)
    read = 0
    while read < length:
        chunk = sock.recv_into(view[read:])
        if chunk == 0:
            raise ConnectionError("socket closed while reading")
        read += chunk
    return bytes(buf)


__all__ = ["HEADER_SIZE", "MAX_PAYLOAD_SIZE", "encode_frame", "read_frame"]
