use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Failed to parse config: {0}")]
    ParseError(#[from] toml::de::Error),

    #[error("Bundle not found: {0}")]
    BundleNotFound(String),

    #[error("Service not found: {0}")]
    ServiceNotFound(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

pub type Result<T> = std::result::Result<T, ConfigError>;

/// Transport-level configuration (channel sizes, buffer sizes, etc.)
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Channel capacity for broadcast channels
    pub channel_capacity: usize,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            channel_capacity: 1024,
        }
    }
}

/// Top-level configuration for Mycelium deployment
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Topology {
    #[serde(default)]
    pub nodes: Vec<Node>,

    /// Socket directory for Unix domain sockets (default: /tmp/mycelium)
    #[serde(default = "default_socket_dir")]
    pub socket_dir: PathBuf,
}

/// Node: a process that runs one or more services
///
/// Transport is inferred from the topology:
/// - Services in same node → Arc<T> (in-memory)
/// - Nodes on same host → Unix sockets
/// - Nodes on different hosts → TCP
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Node {
    pub name: String,
    pub services: Vec<String>,

    /// Optional: hostname (defaults to localhost)
    /// If multiple nodes have same host, they use Unix sockets
    /// If different hosts, they use TCP
    #[serde(default)]
    pub host: Option<String>,

    /// Optional: TCP port (required if host is set and not localhost)
    #[serde(default)]
    pub port: Option<u16>,

    /// Optional: Endpoint configuration for exposing this node's messages
    /// to external clients via socket endpoints
    #[serde(default)]
    pub endpoint: Option<EndpointConfig>,
}

/// Configuration for exposing a node via socket endpoint
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EndpointConfig {
    /// Kind of endpoint: "tcp" or "unix"
    pub kind: EndpointKind,

    /// Address for TCP endpoints (e.g., "127.0.0.1:9091")
    /// Ignored for Unix endpoints
    #[serde(default)]
    pub addr: Option<String>,
}

/// Endpoint kind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EndpointKind {
    Tcp,
    Unix,
}

fn default_socket_dir() -> PathBuf {
    PathBuf::from("/tmp/mycelium")
}

/// Transport type for messages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportType {
    /// In-process Arc<T> (zero-copy)
    Local,

    /// Unix domain sockets (same machine)
    Unix,

    /// TCP sockets (network)
    Tcp,
}

impl Topology {
    /// Load topology from TOML file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let contents = fs::read_to_string(path)?;
        let topology: Topology = toml::from_str(&contents)?;
        topology.validate()?;
        Ok(topology)
    }

    /// Validate the topology configuration
    fn validate(&self) -> Result<()> {
        // Check for duplicate node names
        let mut node_names = std::collections::HashSet::new();
        for node in &self.nodes {
            if !node_names.insert(&node.name) {
                return Err(ConfigError::InvalidConfig(format!(
                    "Duplicate node name: {}",
                    node.name
                )));
            }
        }

        // Check for duplicate service names across nodes
        let mut service_names = std::collections::HashSet::new();
        for node in &self.nodes {
            for service in &node.services {
                if !service_names.insert(service) {
                    return Err(ConfigError::InvalidConfig(format!(
                        "Service {} appears in multiple nodes",
                        service
                    )));
                }
            }
        }

        // Validate that nodes with non-localhost hosts have ports
        for node in &self.nodes {
            if let Some(host) = &node.host {
                if host != "localhost" && host != "127.0.0.1" && node.port.is_none() {
                    return Err(ConfigError::InvalidConfig(format!(
                        "Node {} has remote host '{}' but no port specified",
                        node.name, host
                    )));
                }
            }
        }

        Ok(())
    }

    /// Find which node a service belongs to
    pub fn find_node(&self, service_name: &str) -> Option<&Node> {
        self.nodes
            .iter()
            .find(|n| n.services.iter().any(|s| s == service_name))
    }

    /// Check if two services are in the same node
    pub fn same_node(&self, service1: &str, service2: &str) -> bool {
        if let Some(node1) = self.find_node(service1) {
            if let Some(node2) = self.find_node(service2) {
                return node1.name == node2.name;
            }
        }
        false
    }

    /// Get the transport type for communication between two services
    ///
    /// Transport is inferred from topology:
    /// - Same node → Local (Arc<T>)
    /// - Different nodes, same host → Unix sockets
    /// - Different nodes, different hosts → TCP
    pub fn transport_between(&self, from_service: &str, to_service: &str) -> TransportType {
        // Same node → Local
        if self.same_node(from_service, to_service) {
            return TransportType::Local;
        }

        // Different nodes → check hosts
        if let (Some(node1), Some(node2)) =
            (self.find_node(from_service), self.find_node(to_service))
        {
            match (&node1.host, &node2.host) {
                // Both on localhost or both unspecified → Unix
                (None, None) => TransportType::Unix,
                (Some(h), None) | (None, Some(h)) if h == "localhost" || h == "127.0.0.1" => {
                    TransportType::Unix
                }
                (Some(h1), Some(h2)) if h1 == h2 => TransportType::Unix,

                // Different hosts → TCP
                _ => TransportType::Tcp,
            }
        } else {
            // Fallback if service not found
            TransportType::Local
        }
    }

    /// Get the Unix socket path for a node
    pub fn socket_path(&self, node_name: &str) -> PathBuf {
        self.socket_dir.join(format!("{}.sock", node_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_node() {
        let toml = r#"
            [[nodes]]
            name = "main"
            services = ["service1", "service2"]
        "#;

        let topology: Topology = toml::from_str(toml).unwrap();
        assert_eq!(topology.nodes.len(), 1);
        assert_eq!(topology.nodes[0].name, "main");
        assert_eq!(topology.nodes[0].services.len(), 2);
    }

    #[test]
    fn test_parse_multi_node_config() {
        let toml = r#"
            socket_dir = "/tmp/mycelium"

            [[nodes]]
            name = "adapters"
            services = ["polygon-adapter", "ethereum-adapter"]

            [[nodes]]
            name = "strategies"
            services = ["flash-arbitrage"]
        "#;

        let topology: Topology = toml::from_str(toml).unwrap();
        assert_eq!(topology.nodes.len(), 2);
        assert_eq!(topology.nodes[0].name, "adapters");
        assert_eq!(topology.nodes[0].services.len(), 2);
        assert_eq!(topology.socket_dir, PathBuf::from("/tmp/mycelium"));
    }

    #[test]
    fn test_find_node() {
        let toml = r#"
            [[nodes]]
            name = "adapters"
            services = ["polygon-adapter", "ethereum-adapter"]
        "#;

        let topology: Topology = toml::from_str(toml).unwrap();
        let node = topology.find_node("polygon-adapter").unwrap();
        assert_eq!(node.name, "adapters");
    }

    #[test]
    fn test_same_node() {
        let toml = r#"
            [[nodes]]
            name = "adapters"
            services = ["polygon-adapter", "ethereum-adapter"]

            [[nodes]]
            name = "strategies"
            services = ["flash-arbitrage"]
        "#;

        let topology: Topology = toml::from_str(toml).unwrap();
        assert!(topology.same_node("polygon-adapter", "ethereum-adapter"));
        assert!(!topology.same_node("polygon-adapter", "flash-arbitrage"));
    }

    #[test]
    fn test_transport_between_same_node() {
        let toml = r#"
            [[nodes]]
            name = "adapters"
            services = ["polygon-adapter", "ethereum-adapter"]
        "#;

        let topology: Topology = toml::from_str(toml).unwrap();

        // Same node -> Local
        assert_eq!(
            topology.transport_between("polygon-adapter", "ethereum-adapter"),
            TransportType::Local
        );
    }

    #[test]
    fn test_transport_between_different_nodes_localhost() {
        let toml = r#"
            [[nodes]]
            name = "adapters"
            services = ["polygon-adapter"]

            [[nodes]]
            name = "strategies"
            services = ["flash-arbitrage"]
        "#;

        let topology: Topology = toml::from_str(toml).unwrap();

        // Different nodes, no host specified -> Unix
        assert_eq!(
            topology.transport_between("polygon-adapter", "flash-arbitrage"),
            TransportType::Unix
        );
    }

    #[test]
    fn test_transport_between_different_hosts() {
        let toml = r#"
            [[nodes]]
            name = "adapters"
            services = ["polygon-adapter"]
            host = "10.0.1.10"
            port = 9000

            [[nodes]]
            name = "strategies"
            services = ["flash-arbitrage"]
            host = "10.0.2.20"
            port = 9001
        "#;

        let topology: Topology = toml::from_str(toml).unwrap();

        // Different hosts -> TCP
        assert_eq!(
            topology.transport_between("polygon-adapter", "flash-arbitrage"),
            TransportType::Tcp
        );
    }

    #[test]
    fn test_socket_path() {
        let toml = r#"
            socket_dir = "/tmp/test"

            [[nodes]]
            name = "adapters"
            services = ["polygon-adapter"]
        "#;

        let topology: Topology = toml::from_str(toml).unwrap();
        let path = topology.socket_path("adapters");
        assert_eq!(path, PathBuf::from("/tmp/test/adapters.sock"));
    }

    #[test]
    fn test_validate_duplicate_node_names() {
        let toml = r#"
            [[nodes]]
            name = "test"
            services = ["service1"]

            [[nodes]]
            name = "test"
            services = ["service2"]
        "#;

        let topology: Topology = toml::from_str(toml).unwrap();
        assert!(topology.validate().is_err());
    }

    #[test]
    fn test_validate_duplicate_service_names() {
        let toml = r#"
            [[nodes]]
            name = "node1"
            services = ["service1"]

            [[nodes]]
            name = "node2"
            services = ["service1"]
        "#;

        let topology: Topology = toml::from_str(toml).unwrap();
        assert!(topology.validate().is_err());
    }
}
