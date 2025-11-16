"""Socket transports for the Mycelium Python SDK."""

from __future__ import annotations

import queue
import socket
import struct
import threading
from typing import Dict, List, Optional, Tuple, Type

from mycelium.protocol.runtime import get_message_definition

from .codec import read_frame
from .publisher import Publisher
from .subscriber import Subscriber


class TransportError(Exception):
    pass


class _SocketTransport:
    def __init__(self) -> None:
        self._socket: Optional[socket.socket] = None
        self._send_lock = threading.Lock()
        self._reader_thread: Optional[threading.Thread] = None
        self._running = threading.Event()
        self._subscribers: Dict[int, List[queue.Queue]] = {}
        self._sub_lock = threading.Lock()

    def connect(self) -> None:
        raise NotImplementedError

    def close(self) -> None:
        self._running.clear()
        if self._socket:
            try:
                self._socket.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            self._socket.close()
            self._socket = None
        if self._reader_thread and self._reader_thread.is_alive():
            self._reader_thread.join(timeout=1.0)

    def publisher(self, message_cls: Type) -> Publisher:
        return Publisher(message_cls, self._send_frame)

    def subscriber(self, message_cls: Type) -> Subscriber:
        definition = message_cls.__definition__
        q: queue.Queue = queue.Queue()
        with self._sub_lock:
            self._subscribers.setdefault(definition.type_id, []).append(q)
        return Subscriber(message_cls, q)

    # Internal helpers -------------------------------------------------
    def _send_frame(self, frame: bytes) -> None:
        if not self._socket:
            raise TransportError("transport not connected")
        with self._send_lock:
            self._socket.sendall(frame)

    def _ensure_reader(self) -> None:
        if self._reader_thread and self._reader_thread.is_alive():
            return
        self._running.set()
        thread = threading.Thread(target=self._reader_loop, daemon=True)
        self._reader_thread = thread
        thread.start()

    def _reader_loop(self) -> None:
        assert self._socket is not None
        sock = self._socket
        try:
            while self._running.is_set():
                type_id, payload = read_frame(sock)
                definition = get_message_definition(type_id)
                message = definition.decode(definition.dataclass, payload)
                with self._sub_lock:
                    queues = list(self._subscribers.get(type_id, []))
                for q in queues:
                    q.put(message)
        except (ConnectionError, OSError):
            self._running.clear()


class UnixTransport(_SocketTransport):
    def __init__(self, path: str, schema_digest: bytes) -> None:
        super().__init__()
        self._path = path
        self._schema_digest = schema_digest

    def connect(self) -> None:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.connect(self._path)
        _send_handshake(sock, self._schema_digest)
        self._socket = sock
        self._ensure_reader()


class TcpTransport(_SocketTransport):
    def __init__(self, host: str, port: int, schema_digest: bytes) -> None:
        super().__init__()
        self._host = host
        self._port = port
        self._schema_digest = schema_digest

    def connect(self) -> None:
        sock = socket.create_connection((self._host, self._port))
        _send_handshake(sock, self._schema_digest)
        self._socket = sock
        self._ensure_reader()


def _send_handshake(sock: socket.socket, digest: bytes) -> None:
    packet = struct.pack("<H", len(digest)) + digest
    sock.sendall(packet)



__all__ = ["UnixTransport", "TcpTransport", "TransportError"]
