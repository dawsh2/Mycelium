use crate::any::{AnyPublisher, AnySubscriber};
use crate::config::TransportConfig;
use crate::local::LocalTransport;
use crate::tcp::{TcpPublisher, TcpSubscriber, TcpTransport};
use crate::unix::{UnixPublisher, UnixSubscriber, UnixTransport};
use crate::{Publisher, Result, Subscriber, TransportError};
use mycelium_config::{DeploymentMode, Topology, TransportType};
use mycelium_protocol::Message;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Message bus coordinator
///
/// Provides a unified interface for pub/sub messaging with support for:
/// - Local transport (Arc<T>) for in-process/same-bundle communication
/// - Unix socket transport for inter-bundle communication (same machine)
/// - TCP transport for distributed communication (cross-machine)
/// - Topology-aware transport selection
/// - Configurable performance parameters
pub struct MessageBus {
    /// Local transport for in-bundle communication
    local: LocalTransport,

    /// Transport configuration
    config: TransportConfig,

    /// Optional topology configuration
    topology: Option<Arc<Topology>>,

    /// Name of the bundle this bus belongs to
    bundle_name: Option<String>,

    /// Unix transports to other bundles (lazy-initialized)
    unix_transports: Arc<RwLock<HashMap<String, Arc<UnixTransport>>>>,

    /// TCP transports to remote bundles (lazy-initialized)
    tcp_transports: Arc<RwLock<HashMap<String, Arc<TcpTransport>>>>,
}

impl MessageBus {
    /// Create a new message bus with default configuration
    ///
    /// All messages are passed via Arc<T> for zero-copy performance.
    pub fn new() -> Self {
        Self::with_config(TransportConfig::default())
    }

    /// Create a message bus with custom configuration
    pub fn with_config(config: TransportConfig) -> Self {
        Self {
            local: LocalTransport::with_config(config.clone()),
            config,
            topology: None,
            bundle_name: None,
            unix_transports: Arc::new(RwLock::new(HashMap::new())),
            tcp_transports: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a message bus with custom channel capacity (deprecated)
    ///
    /// Use with_config(TransportConfig) instead for more configuration options.
    #[deprecated(since = "0.2.0", note = "Use with_config(TransportConfig) instead")]
    pub fn with_capacity(capacity: usize) -> Self {
        let mut config = TransportConfig::default();
        config.channel_capacity = capacity;
        Self::with_config(config)
    }

    /// Create a message bus from topology configuration
    ///
    /// This enables hybrid transport: Arc<T> within bundle, Unix/TCP between bundles.
    pub fn from_topology(topology: Topology, bundle_name: impl Into<String>) -> Self {
        Self::from_topology_with_config(topology, bundle_name, TransportConfig::default())
    }

    /// Create a message bus from topology configuration with custom transport config
    pub fn from_topology_with_config(topology: Topology, bundle_name: impl Into<String>, config: TransportConfig) -> Self {
        Self {
            local: LocalTransport::with_config(config.clone()),
            config,
            topology: Some(Arc::new(topology)),
            bundle_name: Some(bundle_name.into()),
            unix_transports: Arc::new(RwLock::new(HashMap::new())),
            tcp_transports: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Generic helper for lazy transport creation with caching
    ///
    /// Checks if transport exists in cache, returns it if found.
    /// Otherwise creates new transport using provided function and caches it.
    async fn get_or_create_transport<T, F, Fut>(
        cache: &RwLock<HashMap<String, Arc<T>>>,
        key: &str,
        create_fn: F,
    ) -> Option<Arc<T>>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Option<T>>,
    {
        // Check cache first (read lock)
        {
            let transports = cache.read().await;
            if let Some(transport) = transports.get(key) {
                return Some(Arc::clone(transport));
            }
        }

        // Create new transport
        let new_transport = create_fn().await?;
        let arc = Arc::new(new_transport);

        // Cache it (write lock)
        cache
            .write()
            .await
            .insert(key.to_string(), Arc::clone(&arc));

        Some(arc)
    }

    /// Get a publisher for a message type (local transport)
    ///
    /// Multiple publishers can exist for the same message type.
    /// All will publish to the same topic.
    pub fn publisher<M: Message>(&self) -> Publisher<M> {
        self.local.publisher()
    }

    /// Get a subscriber for a message type (local transport)
    ///
    /// Each subscriber receives all messages published to the topic.
    /// Subscribers are independent - each gets a copy (via Arc).
    pub fn subscriber<M: Message>(&self) -> Subscriber<M> {
        self.local.subscriber()
    }

    /// Get a Unix publisher to a specific bundle
    ///
    /// Returns None if no topology is configured or bundle not found.
    pub async fn unix_publisher<M: Message>(
        &self,
        target_bundle: &str,
    ) -> Option<UnixPublisher<M>> {
        let topology = self.topology.as_ref()?;
        let socket_path = topology.socket_path(target_bundle);

        let transport =
            Self::get_or_create_transport(&self.unix_transports, target_bundle, || async move {
                UnixTransport::connect(&socket_path).await.ok()
            })
            .await?;

        Some(transport.publisher())
    }

    /// Get a Unix subscriber from a specific bundle
    ///
    /// Returns None if no topology is configured or bundle not found.
    pub async fn unix_subscriber<M: Message>(
        &self,
        source_bundle: &str,
    ) -> Option<UnixSubscriber<M>> {
        let topology = self.topology.as_ref()?;
        let socket_path = topology.socket_path(source_bundle);

        let transport =
            Self::get_or_create_transport(&self.unix_transports, source_bundle, || async move {
                UnixTransport::connect(&socket_path).await.ok()
            })
            .await?;

        Some(transport.subscriber())
    }

    /// Get a TCP publisher to a specific remote bundle
    ///
    /// Returns None if no topology is configured or bundle address not found.
    pub async fn tcp_publisher<M: Message>(&self, target_bundle: &str) -> Option<TcpPublisher<M>> {
        let topology = self.topology.as_ref()?;

        // Find bundle by name and build socket address
        let bundle = topology.bundles.iter().find(|b| b.name == target_bundle)?;
        let host = bundle.host.as_ref()?;
        let port = bundle.port?;
        let addr: SocketAddr = format!("{}:{}", host, port).parse().ok()?;

        let transport =
            Self::get_or_create_transport(&self.tcp_transports, target_bundle, || async move {
                TcpTransport::connect(addr).await.ok()
            })
            .await?;

        Some(transport.publisher())
    }

    /// Get a TCP subscriber from a specific remote bundle
    ///
    /// Returns None if no topology is configured or bundle address not found.
    pub async fn tcp_subscriber<M: Message>(
        &self,
        source_bundle: &str,
    ) -> Option<TcpSubscriber<M>> {
        let topology = self.topology.as_ref()?;

        // Find bundle by name and build socket address
        let bundle = topology.bundles.iter().find(|b| b.name == source_bundle)?;
        let host = bundle.host.as_ref()?;
        let port = bundle.port?;
        let addr: SocketAddr = format!("{}:{}", host, port).parse().ok()?;

        let transport =
            Self::get_or_create_transport(&self.tcp_transports, source_bundle, || async move {
                TcpTransport::connect(addr).await.ok()
            })
            .await?;

        Some(transport.subscriber())
    }

    /// Get a publisher to a specific service with automatic transport selection
    ///
    /// This is the recommended way to get publishers in bundled/distributed deployments.
    /// The transport is automatically selected based on topology configuration:
    /// - Same bundle → Local (Arc<T>)
    /// - Different bundle, same machine → Unix socket
    /// - Different machine → TCP
    ///
    /// Returns an error if:
    /// - No topology is configured (use `publisher()` for monolith mode)
    /// - Target service not found in topology
    /// - Transport initialization fails
    pub async fn publisher_to<M>(&self, target_service: &str) -> Result<AnyPublisher<M>>
    where
        M: Message + rkyv::Serialize<rkyv::ser::serializers::AllocSerializer<1024>>,
    {
        let topology = self.topology.as_ref().ok_or_else(|| {
            TransportError::ServiceNotFound(
                "No topology configured - use publisher() for monolith mode".to_string(),
            )
        })?;

        let my_bundle = self.bundle_name.as_ref().ok_or_else(|| {
            TransportError::ServiceNotFound("No bundle name configured".to_string())
        })?;

        // Find target bundle
        let target_bundle = topology.find_bundle(target_service).ok_or_else(|| {
            TransportError::ServiceNotFound(format!(
                "Service '{}' not found in topology",
                target_service
            ))
        })?;

        // Determine transport type based on bundle comparison
        // (We have bundle names, not service names, so we compare bundles directly)
        let transport = if my_bundle == &target_bundle.name {
            // Same bundle - use local Arc<T>
            TransportType::Local
        } else {
            // Different bundles - check deployment mode
            match topology.deployment.mode {
                DeploymentMode::Monolith => TransportType::Local,
                DeploymentMode::Bundled | DeploymentMode::Isolated => {
                    // Use inter_bundle config (defaults to Unix)
                    topology
                        .inter_bundle
                        .as_ref()
                        .map(|ib| ib.transport)
                        .unwrap_or(TransportType::Unix)
                }
                DeploymentMode::Distributed => {
                    // Find my bundle to compare hosts
                    let my_bundle_obj = topology.bundles.iter().find(|b| &b.name == my_bundle);

                    match (my_bundle_obj, &target_bundle.host) {
                        (Some(mb), Some(target_host)) if mb.host.as_ref() == Some(target_host) => {
                            // Same host - use Unix
                            TransportType::Unix
                        }
                        _ => TransportType::Tcp,
                    }
                }
            }
        };

        match transport {
            TransportType::Local => {
                // Same bundle - use Arc<T>
                Ok(AnyPublisher::Local(self.local.publisher()))
            }
            TransportType::Unix => {
                // Different bundle, same machine
                let pub_ = self
                    .unix_publisher(&target_bundle.name)
                    .await
                    .ok_or_else(|| {
                        TransportError::ServiceNotFound(format!(
                            "Failed to create Unix publisher to '{}'",
                            target_bundle.name
                        ))
                    })?;
                Ok(AnyPublisher::Unix(pub_))
            }
            TransportType::Tcp => {
                // Different machine
                let pub_ = self
                    .tcp_publisher(&target_bundle.name)
                    .await
                    .ok_or_else(|| {
                        TransportError::ServiceNotFound(format!(
                            "Failed to create TCP publisher to '{}'",
                            target_bundle.name
                        ))
                    })?;
                Ok(AnyPublisher::Tcp(pub_))
            }
        }
    }

    /// Get a subscriber from a specific service with automatic transport selection
    ///
    /// This is the recommended way to get subscribers in bundled/distributed deployments.
    /// The transport is automatically selected based on topology configuration:
    /// - Same bundle → Local (Arc<T>)
    /// - Different bundle, same machine → Unix socket
    /// - Different machine → TCP
    ///
    /// Returns an error if:
    /// - No topology is configured (use `subscriber()` for monolith mode)
    /// - Source service not found in topology
    /// - Transport initialization fails
    pub async fn subscriber_from<M>(&self, source_service: &str) -> Result<AnySubscriber<M>>
    where
        M: Message + Clone,
    {
        let topology = self.topology.as_ref().ok_or_else(|| {
            TransportError::ServiceNotFound(
                "No topology configured - use subscriber() for monolith mode".to_string(),
            )
        })?;

        let my_bundle = self.bundle_name.as_ref().ok_or_else(|| {
            TransportError::ServiceNotFound("No bundle name configured".to_string())
        })?;

        // Find source bundle
        let source_bundle = topology.find_bundle(source_service).ok_or_else(|| {
            TransportError::ServiceNotFound(format!(
                "Service '{}' not found in topology",
                source_service
            ))
        })?;

        // Determine transport type based on bundle comparison
        // (We have bundle names, not service names, so we compare bundles directly)
        let transport = if my_bundle == &source_bundle.name {
            // Same bundle - use local Arc<T>
            TransportType::Local
        } else {
            // Different bundles - check deployment mode
            match topology.deployment.mode {
                DeploymentMode::Monolith => TransportType::Local,
                DeploymentMode::Bundled | DeploymentMode::Isolated => {
                    // Use inter_bundle config (defaults to Unix)
                    topology
                        .inter_bundle
                        .as_ref()
                        .map(|ib| ib.transport)
                        .unwrap_or(TransportType::Unix)
                }
                DeploymentMode::Distributed => {
                    // Find my bundle to compare hosts
                    let my_bundle_obj = topology.bundles.iter().find(|b| &b.name == my_bundle);

                    match (my_bundle_obj, &source_bundle.host) {
                        (Some(mb), Some(source_host)) if mb.host.as_ref() == Some(source_host) => {
                            // Same host - use Unix
                            TransportType::Unix
                        }
                        _ => TransportType::Tcp,
                    }
                }
            }
        };

        match transport {
            TransportType::Local => {
                // Same bundle - use Arc<T>
                Ok(AnySubscriber::Local(self.local.subscriber()))
            }
            TransportType::Unix => {
                // Different bundle, same machine
                let sub = self
                    .unix_subscriber(&source_bundle.name)
                    .await
                    .ok_or_else(|| {
                        TransportError::ServiceNotFound(format!(
                            "Failed to create Unix subscriber from '{}'",
                            source_bundle.name
                        ))
                    })?;
                Ok(AnySubscriber::Unix(sub))
            }
            TransportType::Tcp => {
                // Different machine
                let sub = self
                    .tcp_subscriber(&source_bundle.name)
                    .await
                    .ok_or_else(|| {
                        TransportError::ServiceNotFound(format!(
                            "Failed to create TCP subscriber from '{}'",
                            source_bundle.name
                        ))
                    })?;
                Ok(AnySubscriber::Tcp(sub))
            }
        }
    }

    /// Get the number of active subscribers for a message type
    pub fn subscriber_count<M: Message>(&self) -> usize {
        self.local.subscriber_count::<M>()
    }

    /// Get the bundle name this bus belongs to
    pub fn bundle_name(&self) -> Option<&str> {
        self.bundle_name.as_deref()
    }

    /// Get the topology configuration
    pub fn topology(&self) -> Option<&Topology> {
        self.topology.as_deref()
    }
}

impl Default for MessageBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mycelium_protocol::impl_message;
    use rkyv::{Archive, Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Archive, Serialize, Deserialize)]
    #[archive(check_bytes)]
    struct SwapEvent {
        pool: u64,
        amount: u128,
    }

    impl_message!(SwapEvent, 11, "market-data");

    #[tokio::test]
    async fn test_message_bus_basic() {
        let bus = MessageBus::new();

        let pub_ = bus.publisher::<SwapEvent>();
        let mut sub = bus.subscriber::<SwapEvent>();

        pub_.publish(SwapEvent {
            pool: 1,
            amount: 1000,
        })
        .await
        .unwrap();

        let event = sub.recv().await.unwrap();
        assert_eq!(event.pool, 1);
        assert_eq!(event.amount, 1000);
    }

    #[tokio::test]
    async fn test_multiple_publishers() {
        let bus = MessageBus::new();

        let pub1 = bus.publisher::<SwapEvent>();
        let pub2 = bus.publisher::<SwapEvent>();
        let mut sub = bus.subscriber::<SwapEvent>();

        pub1.publish(SwapEvent {
            pool: 1,
            amount: 1000,
        })
        .await
        .unwrap();

        pub2.publish(SwapEvent {
            pool: 2,
            amount: 2000,
        })
        .await
        .unwrap();

        let event1 = sub.recv().await.unwrap();
        let event2 = sub.recv().await.unwrap();

        assert_eq!(event1.pool, 1);
        assert_eq!(event2.pool, 2);
    }

    #[tokio::test]
    async fn test_subscriber_count() {
        let bus = MessageBus::new();

        let _sub1 = bus.subscriber::<SwapEvent>();
        let _sub2 = bus.subscriber::<SwapEvent>();

        assert_eq!(bus.subscriber_count::<SwapEvent>(), 2);
    }

    #[tokio::test]
    async fn test_custom_capacity() {
        let bus = MessageBus::with_capacity(5);

        let pub_ = bus.publisher::<SwapEvent>();
        let mut sub = bus.subscriber::<SwapEvent>();

        pub_.publish(SwapEvent {
            pool: 1,
            amount: 1000,
        })
        .await
        .unwrap();

        let event = sub.recv().await.unwrap();
        assert_eq!(event.pool, 1);
    }

    #[tokio::test]
    async fn test_bundled_deployment() {
        use mycelium_config::{
            Bundle, Deployment, DeploymentMode, InterBundleConfig, Topology, TransportType,
        };

        // Create topology with 2 bundles
        let dir = tempfile::tempdir().unwrap();
        let topology = Topology {
            deployment: Deployment {
                mode: DeploymentMode::Bundled,
            },
            bundles: vec![
                Bundle {
                    name: "adapters".to_string(),
                    services: vec!["polygon-adapter".to_string()],
                    host: None,
                    port: None,
                },
                Bundle {
                    name: "strategies".to_string(),
                    services: vec!["flash-arbitrage".to_string()],
                    host: None,
                    port: None,
                },
            ],
            inter_bundle: Some(InterBundleConfig {
                transport: TransportType::Unix,
                socket_dir: dir.path().to_path_buf(),
            }),
        };

        // Bundle 1: adapters (server side - binds socket)
        let socket_path = topology.socket_path("adapters");
        let _adapter_transport = crate::unix::UnixTransport::bind(&socket_path)
            .await
            .unwrap();

        // Give server time to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Bundle 1: Create message bus for adapters
        let adapters_bus = MessageBus::from_topology(topology.clone(), "adapters");

        // Bundle 1: Publish locally and listen for external subscriptions
        let adapter_pub = adapters_bus.publisher::<SwapEvent>();
        adapter_pub
            .publish(SwapEvent {
                pool: 100,
                amount: 5000,
            })
            .await
            .unwrap();

        // Bundle 2: Create message bus for strategies
        let strategies_bus = MessageBus::from_topology(topology, "strategies");

        // Bundle 2: Get Unix publisher to send to adapters bundle
        let unix_pub = strategies_bus
            .unix_publisher::<SwapEvent>("adapters")
            .await
            .expect("Failed to create Unix publisher");

        // Bundle 2: Publish message to adapters bundle
        unix_pub
            .publish(SwapEvent {
                pool: 200,
                amount: 10000,
            })
            .await
            .unwrap();

        // Verify metadata
        assert_eq!(adapters_bus.bundle_name(), Some("adapters"));
        assert_eq!(strategies_bus.bundle_name(), Some("strategies"));
        assert!(adapters_bus.topology().is_some());
    }

    #[tokio::test]
    async fn test_from_topology() {
        use mycelium_config::{Deployment, DeploymentMode, Topology};

        let topology = Topology {
            deployment: Deployment {
                mode: DeploymentMode::Monolith,
            },
            bundles: vec![],
            inter_bundle: None,
        };

        let bus = MessageBus::from_topology(topology, "test-bundle");

        assert_eq!(bus.bundle_name(), Some("test-bundle"));
        assert!(bus.topology().is_some());
    }

    #[tokio::test]
    async fn test_distributed_deployment() {
        use mycelium_config::{Bundle, Deployment, DeploymentMode, Topology};

        // Create topology with 2 bundles on different hosts
        let topology = Topology {
            deployment: Deployment {
                mode: DeploymentMode::Distributed,
            },
            bundles: vec![
                Bundle {
                    name: "adapters".to_string(),
                    services: vec!["polygon-adapter".to_string()],
                    host: Some("127.0.0.1".to_string()),
                    port: Some(0), // Will be replaced with actual port
                },
                Bundle {
                    name: "strategies".to_string(),
                    services: vec!["flash-arbitrage".to_string()],
                    host: Some("127.0.0.1".to_string()),
                    port: Some(0),
                },
            ],
            inter_bundle: None,
        };

        // Bundle 1: adapters (server side - binds socket)
        let adapter_addr = "127.0.0.1:0".parse().unwrap();
        let adapter_transport = crate::tcp::TcpTransport::bind(adapter_addr).await.unwrap();
        let adapter_bind_addr = adapter_transport.local_addr();

        // Update topology with actual port
        let mut updated_topology = topology.clone();
        updated_topology.bundles[0].port = Some(adapter_bind_addr.port());

        // Give server time to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Bundle 1: Create message bus for adapters
        let adapters_bus = MessageBus::from_topology(updated_topology.clone(), "adapters");

        // Bundle 1: Publish locally
        let adapter_pub = adapters_bus.publisher::<SwapEvent>();
        adapter_pub
            .publish(SwapEvent {
                pool: 100,
                amount: 5000,
            })
            .await
            .unwrap();

        // Bundle 2: Create message bus for strategies
        let strategies_bus = MessageBus::from_topology(updated_topology, "strategies");

        // Bundle 2: Get TCP publisher to send to adapters bundle
        let tcp_pub = strategies_bus
            .tcp_publisher::<SwapEvent>("adapters")
            .await
            .expect("Failed to create TCP publisher");

        // Bundle 2: Publish message to adapters bundle
        tcp_pub
            .publish(SwapEvent {
                pool: 200,
                amount: 10000,
            })
            .await
            .unwrap();

        // Verify metadata
        assert_eq!(adapters_bus.bundle_name(), Some("adapters"));
        assert_eq!(strategies_bus.bundle_name(), Some("strategies"));
        assert!(adapters_bus.topology().is_some());
    }

    #[tokio::test]
    async fn test_publisher_to_same_bundle() {
        use mycelium_config::{Bundle, Deployment, DeploymentMode, Topology};

        // Create topology with services in same bundle
        let topology = Topology {
            deployment: Deployment {
                mode: DeploymentMode::Bundled,
            },
            bundles: vec![Bundle {
                name: "trading".to_string(),
                services: vec!["portfolio-state".to_string(), "order-executor".to_string()],
                host: None,
                port: None,
            }],
            inter_bundle: None,
        };

        let bus = MessageBus::from_topology(topology, "trading");

        // Get publisher to service in same bundle
        let pub_ = bus
            .publisher_to::<SwapEvent>("order-executor")
            .await
            .unwrap();

        // Should use local transport
        assert_eq!(pub_.transport_type(), "local");

        // Verify it works
        let mut sub = bus.subscriber::<SwapEvent>();
        pub_.publish(SwapEvent {
            pool: 1,
            amount: 100,
        })
        .await
        .unwrap();

        let event = sub.recv().await.unwrap();
        assert_eq!(event.pool, 1);
    }

    #[tokio::test]
    async fn test_publisher_to_different_bundle_unix() {
        use mycelium_config::{
            Bundle, Deployment, DeploymentMode, InterBundleConfig, Topology, TransportType,
        };

        let dir = tempfile::tempdir().unwrap();
        let topology = Topology {
            deployment: Deployment {
                mode: DeploymentMode::Bundled,
            },
            bundles: vec![
                Bundle {
                    name: "portfolio".to_string(),
                    services: vec!["portfolio-state".to_string()],
                    host: None,
                    port: None,
                },
                Bundle {
                    name: "executor".to_string(),
                    services: vec!["order-executor".to_string()],
                    host: None,
                    port: None,
                },
            ],
            inter_bundle: Some(InterBundleConfig {
                transport: TransportType::Unix,
                socket_dir: dir.path().to_path_buf(),
            }),
        };

        // Bind server for executor bundle
        let executor_socket = topology.socket_path("executor");
        let _executor_server = crate::unix::UnixTransport::bind(&executor_socket)
            .await
            .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let portfolio_bus = MessageBus::from_topology(topology, "portfolio");

        // Get publisher to different bundle
        let pub_ = portfolio_bus
            .publisher_to::<SwapEvent>("order-executor")
            .await
            .unwrap();

        // Should use Unix transport
        assert_eq!(pub_.transport_type(), "unix");
    }

    #[tokio::test]
    async fn test_publisher_to_distributed_tcp() {
        use mycelium_config::{Bundle, Deployment, DeploymentMode, Topology};

        let topology = Topology {
            deployment: Deployment {
                mode: DeploymentMode::Distributed,
            },
            bundles: vec![
                Bundle {
                    name: "portfolio".to_string(),
                    services: vec!["portfolio-state".to_string()],
                    host: Some("127.0.0.1".to_string()),
                    port: Some(9001),
                },
                Bundle {
                    name: "executor".to_string(),
                    services: vec!["order-executor".to_string()],
                    host: Some("192.168.1.100".to_string()),
                    port: Some(9002),
                },
            ],
            inter_bundle: None,
        };

        let portfolio_bus = MessageBus::from_topology(topology, "portfolio");

        // Get publisher to different host
        let pub_ = portfolio_bus
            .publisher_to::<SwapEvent>("order-executor")
            .await
            .unwrap();

        // Should use TCP transport
        assert_eq!(pub_.transport_type(), "tcp");
    }

    #[tokio::test]
    async fn test_subscriber_from_same_bundle() {
        use mycelium_config::{Bundle, Deployment, DeploymentMode, Topology};

        let topology = Topology {
            deployment: Deployment {
                mode: DeploymentMode::Bundled,
            },
            bundles: vec![Bundle {
                name: "trading".to_string(),
                services: vec!["portfolio-state".to_string(), "order-executor".to_string()],
                host: None,
                port: None,
            }],
            inter_bundle: None,
        };

        let bus = MessageBus::from_topology(topology, "trading");

        // Get subscriber from service in same bundle
        let mut sub = bus
            .subscriber_from::<SwapEvent>("order-executor")
            .await
            .unwrap();

        // Should use local transport
        assert_eq!(sub.transport_type(), "local");

        // Verify it works
        let pub_ = bus.publisher::<SwapEvent>();
        pub_.publish(SwapEvent {
            pool: 2,
            amount: 200,
        })
        .await
        .unwrap();

        let event = sub.recv().await.unwrap();
        assert_eq!(event.pool, 2);
    }

    #[tokio::test]
    async fn test_smart_routing_error_no_topology() {
        let bus = MessageBus::new();

        // Should fail with no topology
        let result = bus.publisher_to::<SwapEvent>("some-service").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_smart_routing_error_service_not_found() {
        use mycelium_config::{Bundle, Deployment, DeploymentMode, Topology};

        let topology = Topology {
            deployment: Deployment {
                mode: DeploymentMode::Bundled,
            },
            bundles: vec![Bundle {
                name: "trading".to_string(),
                services: vec!["portfolio-state".to_string()],
                host: None,
                port: None,
            }],
            inter_bundle: None,
        };

        let bus = MessageBus::from_topology(topology, "trading");

        // Should fail with service not found
        let result = bus.publisher_to::<SwapEvent>("nonexistent-service").await;
        assert!(result.is_err());
    }
}
