"""Mycelium Python SDK."""

from .message_bus import MessageBus
from .publisher import Publisher
from .shm_transport import SharedMemoryTransport, ShmError, ShmReader, ShmWriter
from .subscriber import Subscriber
from .topology import EndpointConfig, EndpointKind, Node, Topology, TransportType
from .transport import TcpTransport, TransportError, UnixTransport

__all__ = [
    # High-level API (recommended)
    "MessageBus",
    "Topology",
    # Topology components
    "Node",
    "TransportType",
    "EndpointKind",
    "EndpointConfig",
    # Low-level transport API
    "TcpTransport",
    "UnixTransport",
    "TransportError",
    "SharedMemoryTransport",
    "ShmError",
    "ShmReader",
    "ShmWriter",
    # Pub/Sub primitives
    "Publisher",
    "Subscriber",
]
