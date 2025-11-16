"""Topology-aware MessageBus for Python SDK."""

from __future__ import annotations

from pathlib import Path
from typing import Dict, Optional, Type

from .publisher import Publisher
from .subscriber import Subscriber
from .topology import Topology, TransportType
from .transport import TcpTransport, TransportError, UnixTransport


class MessageBus:
    """Topology-aware message bus for Python services.

    Provides automatic transport selection based on topology configuration,
    similar to the Rust MessageBus API.

    Example:
        ```python
        from mycelium import MessageBus

        # Load topology and create bus
        bus = MessageBus.from_topology(
            "topology.toml",
            service_name="python-worker",
            schema_digest=SCHEMA_DIGEST
        )

        # Auto-routes based on topology
        pub = bus.publisher_to("target-service", MessageClass)

        # Subscribe locally
        sub = bus.subscriber(MessageClass)
        ```
    """

    def __init__(
        self,
        topology: Topology,
        service_name: str,
        schema_digest: bytes,
    ) -> None:
        """Create a new MessageBus.

        Args:
            topology: Parsed topology configuration
            service_name: Name of this service in the topology
            schema_digest: 32-byte schema digest for handshake validation

        Raises:
            ValueError: If service_name not found in topology
        """
        self._topology = topology
        self._service_name = service_name
        self._schema_digest = schema_digest
        self._transports: Dict[str, UnixTransport | TcpTransport] = {}

        # Validate service exists in topology
        if not self._topology.find_node(service_name):
            raise ValueError(
                f"Service '{service_name}' not found in topology. "
                f"Available services: {self._list_services()}"
            )

    @classmethod
    def from_topology(
        cls,
        topology_path: str | Path,
        service_name: str,
        schema_digest: bytes,
    ) -> MessageBus:
        """Create MessageBus from topology file.

        Args:
            topology_path: Path to topology.toml file
            service_name: Name of this service in the topology
            schema_digest: 32-byte schema digest for validation

        Returns:
            Configured MessageBus instance

        Example:
            ```python
            bus = MessageBus.from_topology(
                "topology.toml",
                "python-worker",
                SCHEMA_DIGEST
            )
            ```
        """
        topology = Topology.load(topology_path)
        return cls(topology, service_name, schema_digest)

    def publisher_to(
        self, target_service: str, message_cls: Type
    ) -> Publisher:
        """Create a publisher to a specific target service.

        Automatically selects the appropriate transport (Unix or TCP)
        based on the topology configuration.

        Args:
            target_service: Name of the target service
            message_cls: Message class to publish

        Returns:
            Publisher for the specified message type

        Raises:
            ValueError: If target service not found or uses LOCAL transport
            TransportError: If connection fails

        Example:
            ```python
            pub = bus.publisher_to("strategy-service", TradeSignal)
            pub.publish(TradeSignal(symbol="BTC", action="buy"))
            ```
        """
        # Get or create transport for this target
        transport = self._get_or_create_transport(target_service)
        return transport.publisher(message_cls)

    def subscriber(self, message_cls: Type) -> Subscriber:
        """Create a subscriber for messages on the local transport.

        Note: This subscribes to the local node's transport. For Python
        services, this means subscribing to messages from the bridge.

        Args:
            message_cls: Message class to subscribe to

        Returns:
            Subscriber for the specified message type

        Raises:
            TransportError: If local transport not connected

        Example:
            ```python
            sub = bus.subscriber(TradeSignal)
            for msg in sub:
                print(f"Received: {msg}")
            ```
        """
        # Python services connect to their own node's endpoint
        transport = self._get_or_create_transport(self._service_name)
        return transport.subscriber(message_cls)

    def close(self) -> None:
        """Close all active transports."""
        for transport in self._transports.values():
            transport.close()
        self._transports.clear()

    # Internal helpers -----------------------------------------------------

    def _get_or_create_transport(
        self, target_service: str
    ) -> UnixTransport | TcpTransport:
        """Get existing transport or create new one for target service.

        Args:
            target_service: Name of service to connect to

        Returns:
            Transport instance (Unix or TCP)

        Raises:
            ValueError: If service not found or transport type not supported
            TransportError: If connection fails
        """
        # Return cached transport if exists
        if target_service in self._transports:
            return self._transports[target_service]

        # Get connection info from topology
        conn_info = self._topology.connection_info(target_service)
        transport_type = conn_info["transport"]

        # Check if LOCAL transport (not supported from Python)
        if transport_type == TransportType.LOCAL:
            raise ValueError(
                f"Cannot connect to service '{target_service}': "
                f"LOCAL transport (Arc<T>) is not available from Python. "
                f"Python services must use Unix or TCP transports."
            )

        # Create appropriate transport
        if transport_type == TransportType.UNIX:
            transport = UnixTransport(
                path=conn_info["path"], schema_digest=self._schema_digest
            )
        elif transport_type == TransportType.TCP:
            transport = TcpTransport(
                host=conn_info["host"],
                port=conn_info["port"],
                schema_digest=self._schema_digest,
            )
        else:
            raise ValueError(f"Unsupported transport type: {transport_type}")

        # Connect and cache
        transport.connect()
        self._transports[target_service] = transport
        return transport

    def _list_services(self) -> list[str]:
        """List all services in topology."""
        services = []
        for node in self._topology.nodes:
            services.extend(node.services)
        return services

    def __enter__(self) -> MessageBus:
        """Context manager entry."""
        return self

    def __exit__(self, exc_type, exc_val, exc_tb) -> None:
        """Context manager exit - closes all transports."""
        self.close()


__all__ = ["MessageBus"]
