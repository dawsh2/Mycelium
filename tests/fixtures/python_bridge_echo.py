#!/usr/bin/env python3
"""Python SDK smoke test used by Rust integration tests."""

from __future__ import annotations

import json
import os
import queue
import sys
import time

from mycelium import UnixTransport
from mycelium_protocol import TextMessage


def dataclass_to_dict(message: TextMessage | None):
    if message is None:
        return None
    return {
        "sender": message.sender,
        "content": message.content,
        "timestamp": int(message.timestamp),
    }


def main() -> int:
    socket_path = os.environ["MYCELIUM_SOCKET"]
    digest_hex = os.environ["MYCELIUM_SCHEMA_DIGEST"]
    output_path = os.environ["MYCELIUM_TEST_OUTPUT"]
    digest = bytes.fromhex(digest_hex)

    transport = UnixTransport(socket_path, digest)
    transport.connect()

    publisher = transport.publisher(TextMessage)
    subscriber = transport.subscriber(TextMessage)

    outbound = TextMessage(
        sender="python-writer",
        content="hello-from-python",
        timestamp=int(time.time()),
    )
    publisher.publish(outbound)

    inbound = None
    deadline = time.time() + 5.0
    while time.time() < deadline:
        try:
            candidate = subscriber.recv(timeout=1.0)
        except queue.Empty:
            continue
        if _is_same_message(candidate, outbound):
            continue
        inbound = candidate
        break

    payload = {
        "outbound": dataclass_to_dict(outbound),
        "inbound": dataclass_to_dict(inbound),
    }

    with open(output_path, "w", encoding="utf-8") as handle:
        json.dump(payload, handle)

    transport.close()
    return 0


def _is_same_message(left: TextMessage, right: TextMessage) -> bool:
    return (
        left.sender == right.sender
        and left.content == right.content
        and int(left.timestamp) == int(right.timestamp)
    )


if __name__ == "__main__":
    sys.exit(main())
