"""Topology configuration for Mycelium Python SDK."""

from __future__ import annotations

import sys
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Dict, List, Optional

# Use tomllib (Python 3.11+) or fallback to tomli
if sys.version_info >= (3, 11):
    import tomllib
else:
    try:
        import tomli as tomllib
    except ImportError:
        raise ImportError(
            "Python <3.11 requires 'tomli' package. "
            "Install with: pip install tomli"
        )


class TransportType(Enum):
    """Transport type for message delivery."""

    LOCAL = "local"  # In-process Arc<T> (not available for Python)
    UNIX = "unix"  # Unix domain sockets
    TCP = "tcp"  # TCP sockets


class EndpointKind(Enum):
    """Endpoint kind for socket endpoints."""

    TCP = "tcp"
    UNIX = "unix"


@dataclass
class EndpointConfig:
    """Configuration for exposing a node via socket endpoint."""

    kind: EndpointKind
    addr: Optional[str] = None


@dataclass
class Node:
    """Node: a process that runs one or more services.

    Transport is inferred from topology:
    - Services in same node → Local (Arc<T>, Rust only)
    - Nodes on same host → Unix sockets
    - Nodes on different hosts → TCP
    """

    name: str
    services: List[str]
    host: Optional[str] = None
    port: Optional[int] = None
    endpoint: Optional[EndpointConfig] = None


@dataclass
class Topology:
    """Top-level configuration for Mycelium deployment.

    Supports loading from TOML files and querying transport types
    between services based on topology.
    """

    nodes: List[Node]
    socket_dir: Path = Path("/tmp/mycelium")

    @classmethod
    def load(cls, path: str | Path) -> Topology:
        """Load topology from TOML file.

        Args:
            path: Path to topology.toml file

        Returns:
            Parsed and validated Topology

        Raises:
            ValueError: If topology is invalid
            FileNotFoundError: If file doesn't exist
        """
        path = Path(path)
        with open(path, "rb") as f:
            data = tomllib.load(f)

        nodes = []
        for node_data in data.get("nodes", []):
            endpoint_data = node_data.get("endpoint")
            endpoint = None
            if endpoint_data:
                kind = EndpointKind(endpoint_data["kind"])
                endpoint = EndpointConfig(
                    kind=kind, addr=endpoint_data.get("addr")
                )

            nodes.append(
                Node(
                    name=node_data["name"],
                    services=node_data["services"],
                    host=node_data.get("host"),
                    port=node_data.get("port"),
                    endpoint=endpoint,
                )
            )

        socket_dir = Path(data.get("socket_dir", "/tmp/mycelium"))
        topology = cls(nodes=nodes, socket_dir=socket_dir)
        topology.validate()
        return topology

    def validate(self) -> None:
        """Validate topology configuration.

        Raises:
            ValueError: If topology is invalid
        """
        # Check for duplicate node names
        node_names = set()
        for node in self.nodes:
            if node.name in node_names:
                raise ValueError(f"Duplicate node name: {node.name}")
            node_names.add(node.name)

        # Check for duplicate service names across nodes
        service_names = set()
        for node in self.nodes:
            for service in node.services:
                if service in service_names:
                    raise ValueError(
                        f"Service {service} appears in multiple nodes"
                    )
                service_names.add(service)

        # Validate nodes with remote hosts have ports
        for node in self.nodes:
            if node.host and node.host not in ("localhost", "127.0.0.1"):
                if node.port is None:
                    raise ValueError(
                        f"Node {node.name} has remote host "
                        f"'{node.host}' but no port specified"
                    )

    def find_node(self, service_name: str) -> Optional[Node]:
        """Find which node a service belongs to.

        Args:
            service_name: Name of the service to find

        Returns:
            Node containing the service, or None if not found
        """
        for node in self.nodes:
            if service_name in node.services:
                return node
        return None

    def same_node(self, service1: str, service2: str) -> bool:
        """Check if two services are in the same node.

        Args:
            service1: First service name
            service2: Second service name

        Returns:
            True if both services are in the same node
        """
        node1 = self.find_node(service1)
        node2 = self.find_node(service2)
        if node1 and node2:
            return node1.name == node2.name
        return False

    def transport_between(
        self, from_service: str, to_service: str
    ) -> TransportType:
        """Get the transport type for communication between two services.

        Transport is inferred from topology:
        - Same node → Local (Arc<T>, not available for Python)
        - Different nodes, same host → Unix sockets
        - Different nodes, different hosts → TCP

        Args:
            from_service: Source service name
            to_service: Target service name

        Returns:
            Transport type to use for communication
        """
        # Same node → Local (not usable from Python)
        if self.same_node(from_service, to_service):
            return TransportType.LOCAL

        # Different nodes → check hosts
        node1 = self.find_node(from_service)
        node2 = self.find_node(to_service)

        if node1 and node2:
            h1 = node1.host
            h2 = node2.host

            # Both on localhost or both unspecified → Unix
            if h1 is None and h2 is None:
                return TransportType.UNIX

            localhost_vals = ("localhost", "127.0.0.1")
            if (h1 in localhost_vals and h2 is None) or (
                h1 is None and h2 in localhost_vals
            ):
                return TransportType.UNIX

            if h1 == h2:
                return TransportType.UNIX

            # Different hosts → TCP
            return TransportType.TCP

        # Fallback
        return TransportType.LOCAL

    def socket_path(self, node_name: str) -> Path:
        """Get the Unix socket path for a node.

        Args:
            node_name: Name of the node

        Returns:
            Path to Unix socket for this node
        """
        return self.socket_dir / f"{node_name}.sock"

    def connection_info(self, target_service: str) -> Dict[str, any]:
        """Get connection information for connecting to a target service.

        Returns a dictionary with transport type and connection details.

        Args:
            target_service: Name of service to connect to

        Returns:
            Dict with keys:
            - 'transport': TransportType enum value
            - 'path': Unix socket path (if Unix transport)
            - 'host': TCP host (if TCP transport)
            - 'port': TCP port (if TCP transport)

        Raises:
            ValueError: If service not found in topology
        """
        node = self.find_node(target_service)
        if not node:
            raise ValueError(f"Service '{target_service}' not found in topology")

        # Determine transport type based on node configuration
        if node.host and node.host not in ("localhost", "127.0.0.1"):
            # Remote node → TCP
            return {
                "transport": TransportType.TCP,
                "host": node.host,
                "port": node.port,
            }
        else:
            # Local node → Unix socket
            return {
                "transport": TransportType.UNIX,
                "path": str(self.socket_path(node.name)),
            }


__all__ = [
    "Topology",
    "Node",
    "TransportType",
    "EndpointKind",
    "EndpointConfig",
]
