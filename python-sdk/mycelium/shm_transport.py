"""Shared memory transport for low-latency IPC.

Provides ~100-500ns latency cross-process communication using mmap-backed
ring buffers. Much faster than Unix sockets (~1-10μs) while maintaining
process isolation.
"""

from __future__ import annotations

import mmap
import os
import queue
import struct
import threading
import time
from pathlib import Path
from typing import Dict, List, Optional, Tuple, Type

from mycelium.protocol.runtime import get_message_definition

from .codec import read_frame
from .publisher import Publisher
from .subscriber import Subscriber

# Constants matching Rust implementation
HEADER_SIZE = 64  # ShmHeader size
PROTOCOL_VERSION = 1
FRAME_HEADER_SIZE = 8  # FrameHeader size (length + type_id + sequence)


class ShmError(Exception):
    """Errors in shared memory operations."""

    pass


class ShmHeader:
    """Shared memory header (64 bytes, cache-line aligned).

    Layout:
    - write_index: u64 (8 bytes)
    - read_index: u64 (8 bytes)
    - capacity: u64 (8 bytes)
    - schema_digest: [u8; 32] (32 bytes)
    - version: u32 (4 bytes)
    - flags: u32 (4 bytes)
    """

    STRUCT_FORMAT = "<QQQ32sII"  # Little-endian, aligned
    STRUCT_SIZE = struct.calcsize(STRUCT_FORMAT)

    def __init__(self, mm: mmap.mmap):
        self.mm = mm

    @property
    def write_index(self) -> int:
        """Get write index (atomically)."""
        self.mm.seek(0)
        return struct.unpack("<Q", self.mm.read(8))[0]

    @write_index.setter
    def write_index(self, value: int):
        """Set write index (atomically)."""
        self.mm.seek(0)
        self.mm.write(struct.pack("<Q", value))

    @property
    def read_index(self) -> int:
        """Get read index (atomically)."""
        self.mm.seek(8)
        return struct.unpack("<Q", self.mm.read(8))[0]

    @read_index.setter
    def read_index(self, value: int):
        """Set read index (atomically)."""
        self.mm.seek(8)
        self.mm.write(struct.pack("<Q", value))

    @property
    def capacity(self) -> int:
        """Get buffer capacity."""
        self.mm.seek(16)
        return struct.unpack("<Q", self.mm.read(8))[0]

    @property
    def schema_digest(self) -> bytes:
        """Get schema digest."""
        self.mm.seek(24)
        return self.mm.read(32)

    @property
    def version(self) -> int:
        """Get protocol version."""
        self.mm.seek(56)
        return struct.unpack("<I", self.mm.read(4))[0]

    def initialize(
        self, capacity: int, schema_digest: bytes, version: int = PROTOCOL_VERSION
    ):
        """Initialize header (writer only)."""
        if len(schema_digest) != 32:
            raise ValueError("Schema digest must be 32 bytes")

        self.mm.seek(0)
        data = struct.pack(
            self.STRUCT_FORMAT,
            0,  # write_index
            0,  # read_index
            capacity,
            schema_digest,
            version,
            0,  # flags
        )
        self.mm.write(data)


class ShmWriter:
    """Shared memory ring buffer writer (producer).

    Creates and initializes a shared memory region for writing frames.
    """

    def __init__(
        self, path: str | Path, schema_digest: bytes, capacity: int = 1024 * 1024
    ):
        """Create a new shared memory writer.

        Args:
            path: Path to shared memory file
            schema_digest: 32-byte schema digest
            capacity: Ring buffer size in bytes (default: 1MB)
        """
        self.path = Path(path)
        self.capacity = capacity
        self.total_size = HEADER_SIZE + capacity
        self.sequence = 0

        # Create parent directory
        self.path.parent.mkdir(parents=True, exist_ok=True)

        # Create or truncate file
        self.fd = os.open(
            self.path, os.O_RDWR | os.O_CREAT | os.O_TRUNC, mode=0o600
        )
        os.ftruncate(self.fd, self.total_size)

        # Memory map the file
        self.mm = mmap.mmap(self.fd, self.total_size, access=mmap.ACCESS_WRITE)
        self.header = ShmHeader(self.mm)
        self.header.initialize(capacity, schema_digest)

        # Cache write pointer offset
        self.write_offset = HEADER_SIZE

    def write_frame(self, type_id: int, payload: bytes) -> None:
        """Write a frame to the ring buffer.

        Args:
            type_id: Message type ID (u16)
            payload: Message payload bytes

        Raises:
            ShmError: If buffer is full
        """
        frame_size = FRAME_HEADER_SIZE + len(payload)

        # Get current indices (atomic loads)
        write_idx = self.header.write_index
        read_idx = self.header.read_index

        # Check available space
        used = write_idx - read_idx
        available = self.capacity - used

        if available < frame_size:
            raise ShmError(
                f"Buffer full (available: {available}, needed: {frame_size})"
            )

        # Write frame header
        pos = (write_idx % self.capacity) + HEADER_SIZE
        frame_header = struct.pack(
            "<IHH",
            frame_size,  # length (u32)
            type_id,  # type_id (u16)
            self.sequence & 0xFFFF,  # sequence (u16)
        )
        self._write_wrapping(pos, frame_header)

        # Write payload
        payload_pos = (pos + FRAME_HEADER_SIZE - HEADER_SIZE) % self.capacity + HEADER_SIZE
        self._write_wrapping(payload_pos, payload)

        # Publish write (atomic store with release semantics)
        self.header.write_index = write_idx + frame_size
        self.sequence += 1

    def has_space(self, frame_size: int) -> bool:
        """Check if buffer has space for a frame."""
        write_idx = self.header.write_index
        read_idx = self.header.read_index
        used = write_idx - read_idx
        available = self.capacity - used
        return available >= frame_size

    def _write_wrapping(self, pos: int, data: bytes):
        """Write data with wraparound handling."""
        end = pos + len(data)
        if end <= HEADER_SIZE + self.capacity:
            # No wraparound
            self.mm.seek(pos)
            self.mm.write(data)
        else:
            # Wraparound
            first_chunk = (HEADER_SIZE + self.capacity) - pos
            second_chunk = len(data) - first_chunk
            self.mm.seek(pos)
            self.mm.write(data[:first_chunk])
            self.mm.seek(HEADER_SIZE)
            self.mm.write(data[first_chunk:])

    def close(self):
        """Close the shared memory writer."""
        if hasattr(self, "mm"):
            self.mm.close()
        if hasattr(self, "fd"):
            os.close(self.fd)


class ShmReader:
    """Shared memory ring buffer reader (consumer).

    Opens an existing shared memory region for reading frames.
    """

    def __init__(self, path: str | Path, expected_digest: bytes):
        """Open an existing shared memory region.

        Args:
            path: Path to shared memory file
            expected_digest: Expected 32-byte schema digest

        Raises:
            ShmError: If digest mismatch or invalid header
        """
        self.path = Path(path)
        self.expected_sequence = 0

        # Open file (read-write for mmap)
        self.fd = os.open(self.path, os.O_RDWR)

        # Memory map the file
        file_size = os.fstat(self.fd).st_size
        self.mm = mmap.mmap(self.fd, file_size, access=mmap.ACCESS_WRITE)
        self.header = ShmHeader(self.mm)

        # Validate header
        if self.header.schema_digest != expected_digest:
            raise ShmError("Schema digest mismatch")

        if self.header.version != PROTOCOL_VERSION:
            raise ShmError(
                f"Unsupported protocol version: {self.header.version}"
            )

        self.capacity = self.header.capacity
        self.read_offset = HEADER_SIZE

    def read_frame(self) -> Optional[Tuple[int, bytes]]:
        """Try to read the next frame.

        Returns:
            (type_id, payload) if frame available, None otherwise

        Raises:
            ShmError: If frame is corrupted
        """
        # Get current indices (atomic loads)
        write_idx = self.header.write_index
        read_idx = self.header.read_index

        # Check if data available
        if write_idx == read_idx:
            return None  # Empty

        # Read frame header
        pos = (read_idx % self.capacity) + HEADER_SIZE
        header_bytes = self._read_wrapping(pos, FRAME_HEADER_SIZE)
        frame_length, type_id, sequence = struct.unpack("<IHH", header_bytes)

        # Validate sequence
        if sequence != (self.expected_sequence & 0xFFFF):
            raise ShmError("Corrupted frame (sequence mismatch)")

        # Read payload
        payload_len = frame_length - FRAME_HEADER_SIZE
        payload_pos = (pos + FRAME_HEADER_SIZE - HEADER_SIZE) % self.capacity + HEADER_SIZE
        payload = self._read_wrapping(payload_pos, payload_len)

        # Publish read (atomic store with release semantics)
        self.header.read_index = read_idx + frame_length
        self.expected_sequence += 1

        return (type_id, payload)

    def _read_wrapping(self, pos: int, length: int) -> bytes:
        """Read data with wraparound handling."""
        end = pos + length
        if end <= HEADER_SIZE + self.capacity:
            # No wraparound
            self.mm.seek(pos)
            return self.mm.read(length)
        else:
            # Wraparound
            first_chunk = (HEADER_SIZE + self.capacity) - pos
            second_chunk = length - first_chunk
            self.mm.seek(pos)
            data = self.mm.read(first_chunk)
            self.mm.seek(HEADER_SIZE)
            data += self.mm.read(second_chunk)
            return data

    def close(self):
        """Close the shared memory reader."""
        if hasattr(self, "mm"):
            self.mm.close()
        if hasattr(self, "fd"):
            os.close(self.fd)


class SharedMemoryTransport:
    """Shared memory transport for low-latency IPC.

    Provides pub/sub over shared memory ring buffers. Achieves ~100-500ns
    latency vs ~1-10μs for Unix sockets.

    Example:
        ```python
        # Writer side (Rust creates, Python can also create)
        transport = SharedMemoryTransport.create(
            "/dev/shm/mycelium/service.shm",
            schema_digest=SCHEMA_DIGEST
        )

        # Reader side
        transport = SharedMemoryTransport.open(
            "/dev/shm/mycelium/service.shm",
            schema_digest=SCHEMA_DIGEST
        )

        pub = transport.publisher(Message)
        sub = transport.subscriber(Message)
        ```
    """

    def __init__(self):
        self._writer: Optional[ShmWriter] = None
        self._reader: Optional[ShmReader] = None
        self._reader_thread: Optional[threading.Thread] = None
        self._running = threading.Event()
        self._subscribers: Dict[int, List[queue.Queue]] = {}
        self._sub_lock = threading.Lock()

    @classmethod
    def create(
        cls,
        path: str | Path,
        schema_digest: bytes,
        capacity: int = 1024 * 1024,
    ) -> SharedMemoryTransport:
        """Create a new shared memory transport (writer + reader).

        Args:
            path: Path to shared memory file
            schema_digest: 32-byte schema digest
            capacity: Ring buffer size (default: 1MB)
        """
        transport = cls()
        transport._writer = ShmWriter(path, schema_digest, capacity)
        transport._reader = ShmReader(path, schema_digest)
        transport._start_reader()
        return transport

    @classmethod
    def open(
        cls, path: str | Path, schema_digest: bytes
    ) -> SharedMemoryTransport:
        """Open an existing shared memory transport (reader only).

        Args:
            path: Path to shared memory file
            schema_digest: Expected 32-byte schema digest
        """
        transport = cls()
        transport._reader = ShmReader(path, schema_digest)
        transport._start_reader()
        return transport

    def publisher(self, message_cls: Type) -> Publisher:
        """Create a publisher for the given message type."""
        if not self._writer:
            raise ShmError("Transport not configured for writing")
        return Publisher(message_cls, self._send_frame)

    def subscriber(self, message_cls: Type) -> Subscriber:
        """Create a subscriber for the given message type."""
        definition = message_cls.__definition__
        q: queue.Queue = queue.Queue()
        with self._sub_lock:
            self._subscribers.setdefault(definition.type_id, []).append(q)
        return Subscriber(message_cls, q)

    def close(self):
        """Close the transport and cleanup resources."""
        self._running.clear()
        if self._reader_thread and self._reader_thread.is_alive():
            self._reader_thread.join(timeout=1.0)
        if self._writer:
            self._writer.close()
        if self._reader:
            self._reader.close()

    # Internal helpers -----------------------------------------------------

    def _send_frame(self, frame: bytes) -> None:
        """Send a TLV frame (called by Publisher)."""
        if not self._writer:
            raise ShmError("Transport not configured for writing")

        # Extract type_id and payload from TLV frame
        if len(frame) < 6:
            raise ValueError("Frame too short")

        type_id = struct.unpack("<H", frame[0:2])[0]
        payload_len = struct.unpack("<I", frame[2:6])[0]
        payload = frame[6 : 6 + payload_len]

        self._writer.write_frame(type_id, payload)

    def _start_reader(self):
        """Start background reader thread."""
        if not self._reader:
            return
        self._running.set()
        thread = threading.Thread(target=self._reader_loop, daemon=True)
        self._reader_thread = thread
        thread.start()

    def _reader_loop(self):
        """Background reader loop."""
        assert self._reader is not None

        while self._running.is_set():
            try:
                result = self._reader.read_frame()
                if result is None:
                    # No frames available, sleep briefly
                    time.sleep(0.00001)  # 10μs
                    continue

                type_id, payload = result
                definition = get_message_definition(type_id)
                message = definition.decode(definition.dataclass, payload)

                with self._sub_lock:
                    queues = list(self._subscribers.get(type_id, []))
                for q in queues:
                    q.put(message)

            except ShmError as e:
                print(f"Shared memory read error: {e}")
                break
            except Exception as e:
                print(f"Unexpected error in reader loop: {e}")
                break


__all__ = ["SharedMemoryTransport", "ShmError", "ShmWriter", "ShmReader"]
