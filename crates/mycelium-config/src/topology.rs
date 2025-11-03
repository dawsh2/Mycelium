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

/// Top-level configuration for Mycelium deployment
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Topology {
    pub deployment: Deployment,

    #[serde(default)]
    pub bundles: Vec<Bundle>,

    #[serde(default)]
    pub inter_bundle: Option<InterBundleConfig>,
}

/// Deployment mode configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Deployment {
    pub mode: DeploymentMode,
}

/// Deployment modes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeploymentMode {
    /// All services in one process (Arc<T>)
    Monolith,

    /// Services grouped in bundles (Arc within, Unix/TCP between)
    Bundled,

    /// Each service in its own process (Unix/TCP all)
    Isolated,

    /// Services across machines (TCP)
    Distributed,
}

/// Bundle of services that run in the same process
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Bundle {
    pub name: String,
    pub services: Vec<String>,

    /// Optional: hostname for distributed deployment
    #[serde(default)]
    pub host: Option<String>,

    /// Optional: TCP port for distributed deployment
    #[serde(default)]
    pub port: Option<u16>,
}

/// Configuration for inter-bundle communication
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InterBundleConfig {
    pub transport: TransportType,

    #[serde(default = "default_socket_dir")]
    pub socket_dir: PathBuf,
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
        // Check for duplicate bundle names
        let mut bundle_names = std::collections::HashSet::new();
        for bundle in &self.bundles {
            if !bundle_names.insert(&bundle.name) {
                return Err(ConfigError::InvalidConfig(format!(
                    "Duplicate bundle name: {}",
                    bundle.name
                )));
            }
        }

        // Check for duplicate service names across bundles
        let mut service_names = std::collections::HashSet::new();
        for bundle in &self.bundles {
            for service in &bundle.services {
                if !service_names.insert(service) {
                    return Err(ConfigError::InvalidConfig(format!(
                        "Service {} appears in multiple bundles",
                        service
                    )));
                }
            }
        }

        // Validate distributed deployment config
        if self.deployment.mode == DeploymentMode::Distributed {
            for bundle in &self.bundles {
                if bundle.host.is_some() && bundle.port.is_none() {
                    return Err(ConfigError::InvalidConfig(format!(
                        "Bundle {} has host but no port",
                        bundle.name
                    )));
                }
                if bundle.host.is_none() && bundle.port.is_some() {
                    return Err(ConfigError::InvalidConfig(format!(
                        "Bundle {} has port but no host",
                        bundle.name
                    )));
                }
            }
        }

        Ok(())
    }

    /// Find which bundle a service belongs to
    pub fn find_bundle(&self, service_name: &str) -> Option<&Bundle> {
        self.bundles
            .iter()
            .find(|b| b.services.iter().any(|s| s == service_name))
    }

    /// Check if two services are in the same bundle
    pub fn same_bundle(&self, service1: &str, service2: &str) -> bool {
        if let Some(bundle1) = self.find_bundle(service1) {
            if let Some(bundle2) = self.find_bundle(service2) {
                return bundle1.name == bundle2.name;
            }
        }
        false
    }

    /// Get the transport type for communication between two services
    pub fn transport_between(&self, from_service: &str, to_service: &str) -> TransportType {
        match self.deployment.mode {
            DeploymentMode::Monolith => TransportType::Local,

            DeploymentMode::Bundled => {
                if self.same_bundle(from_service, to_service) {
                    TransportType::Local
                } else {
                    self.inter_bundle
                        .as_ref()
                        .map(|ib| ib.transport)
                        .unwrap_or(TransportType::Unix)
                }
            }

            DeploymentMode::Isolated => {
                self.inter_bundle
                    .as_ref()
                    .map(|ib| ib.transport)
                    .unwrap_or(TransportType::Unix)
            }

            DeploymentMode::Distributed => {
                // Check if services are on the same host
                if let (Some(bundle1), Some(bundle2)) = (
                    self.find_bundle(from_service),
                    self.find_bundle(to_service),
                ) {
                    match (&bundle1.host, &bundle2.host) {
                        (Some(h1), Some(h2)) if h1 == h2 => TransportType::Unix,
                        (None, None) => TransportType::Local,
                        _ => TransportType::Tcp,
                    }
                } else {
                    TransportType::Local
                }
            }
        }
    }

    /// Get the Unix socket path for a bundle
    pub fn socket_path(&self, bundle_name: &str) -> PathBuf {
        let socket_dir = self
            .inter_bundle
            .as_ref()
            .map(|ib| ib.socket_dir.clone())
            .unwrap_or_else(default_socket_dir);

        socket_dir.join(format!("{}.sock", bundle_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_monolith_config() {
        let toml = r#"
            [deployment]
            mode = "monolith"
        "#;

        let topology: Topology = toml::from_str(toml).unwrap();
        assert_eq!(topology.deployment.mode, DeploymentMode::Monolith);
    }

    #[test]
    fn test_parse_bundled_config() {
        let toml = r#"
            [deployment]
            mode = "bundled"

            [[bundles]]
            name = "adapters"
            services = ["polygon-adapter", "ethereum-adapter"]

            [[bundles]]
            name = "strategies"
            services = ["flash-arbitrage"]

            [inter_bundle]
            transport = "unix"
            socket_dir = "/tmp/mycelium"
        "#;

        let topology: Topology = toml::from_str(toml).unwrap();
        assert_eq!(topology.deployment.mode, DeploymentMode::Bundled);
        assert_eq!(topology.bundles.len(), 2);
        assert_eq!(topology.bundles[0].name, "adapters");
        assert_eq!(topology.bundles[0].services.len(), 2);
    }

    #[test]
    fn test_find_bundle() {
        let toml = r#"
            [deployment]
            mode = "bundled"

            [[bundles]]
            name = "adapters"
            services = ["polygon-adapter", "ethereum-adapter"]
        "#;

        let topology: Topology = toml::from_str(toml).unwrap();
        let bundle = topology.find_bundle("polygon-adapter").unwrap();
        assert_eq!(bundle.name, "adapters");
    }

    #[test]
    fn test_same_bundle() {
        let toml = r#"
            [deployment]
            mode = "bundled"

            [[bundles]]
            name = "adapters"
            services = ["polygon-adapter", "ethereum-adapter"]

            [[bundles]]
            name = "strategies"
            services = ["flash-arbitrage"]
        "#;

        let topology: Topology = toml::from_str(toml).unwrap();
        assert!(topology.same_bundle("polygon-adapter", "ethereum-adapter"));
        assert!(!topology.same_bundle("polygon-adapter", "flash-arbitrage"));
    }

    #[test]
    fn test_transport_between_bundled() {
        let toml = r#"
            [deployment]
            mode = "bundled"

            [[bundles]]
            name = "adapters"
            services = ["polygon-adapter", "ethereum-adapter"]

            [[bundles]]
            name = "strategies"
            services = ["flash-arbitrage"]

            [inter_bundle]
            transport = "unix"
        "#;

        let topology: Topology = toml::from_str(toml).unwrap();

        // Same bundle -> local
        assert_eq!(
            topology.transport_between("polygon-adapter", "ethereum-adapter"),
            TransportType::Local
        );

        // Different bundles -> unix
        assert_eq!(
            topology.transport_between("polygon-adapter", "flash-arbitrage"),
            TransportType::Unix
        );
    }

    #[test]
    fn test_socket_path() {
        let toml = r#"
            [deployment]
            mode = "bundled"

            [[bundles]]
            name = "adapters"
            services = ["polygon-adapter"]

            [inter_bundle]
            transport = "unix"
            socket_dir = "/tmp/test"
        "#;

        let topology: Topology = toml::from_str(toml).unwrap();
        let path = topology.socket_path("adapters");
        assert_eq!(path, PathBuf::from("/tmp/test/adapters.sock"));
    }

    #[test]
    fn test_validate_duplicate_bundle_names() {
        let toml = r#"
            [deployment]
            mode = "bundled"

            [[bundles]]
            name = "test"
            services = ["service1"]

            [[bundles]]
            name = "test"
            services = ["service2"]
        "#;

        let topology: Topology = toml::from_str(toml).unwrap();
        assert!(topology.validate().is_err());
    }

    #[test]
    fn test_validate_duplicate_service_names() {
        let toml = r#"
            [deployment]
            mode = "bundled"

            [[bundles]]
            name = "bundle1"
            services = ["service1"]

            [[bundles]]
            name = "bundle2"
            services = ["service1"]
        "#;

        let topology: Topology = toml::from_str(toml).unwrap();
        assert!(topology.validate().is_err());
    }
}
