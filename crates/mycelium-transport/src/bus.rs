use crate::any::{AnyPublisher, AnySubscriber};
use crate::bounded::{BoundedPublisher, BoundedSubscriber};
use crate::config::{EndpointKind, Node, Topology, TransportConfig, TransportType};
use crate::local::LocalTransport;
use crate::socket_endpoint::{bind_tcp_endpoint, bind_unix_endpoint, SocketEndpointHandle};
use crate::tcp::{TcpPublisher, TcpSubscriber, TcpTransport};
use crate::unix::{UnixPublisher, UnixSubscriber, UnixTransport};
use crate::{Publisher, Result, Subscriber, TransportError};
use mycelium_protocol::Message;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Message bus coordinator
///
/// Provides a unified interface for pub/sub messaging with support for:
/// - Local transport (Arc<T>) for in-process/same-node communication
/// - Unix socket transport for inter-node communication (same machine)
/// - TCP transport for distributed communication (cross-machine)
/// - Topology-aware transport selection
/// - Configurable performance parameters
#[derive(Clone)]
pub struct MessageBus {
    /// Local transport for in-node communication
    local: LocalTransport,

    /// Transport configuration (reserved for future transport-level config)
    #[allow(dead_code)]
    config: TransportConfig,

    /// Optional topology configuration
    topology: Option<Arc<Topology>>,

    /// Name of the node this bus belongs to
    node_name: Option<String>,

    /// Unix transports to other nodes (lazy-initialized)
    unix_transports: Arc<RwLock<HashMap<String, Arc<UnixTransport>>>>,

    /// TCP transports to remote nodes (lazy-initialized)
    tcp_transports: Arc<RwLock<HashMap<String, Arc<TcpTransport>>>>,
}

impl MessageBus {
    /// Create a new message bus with default configuration
    ///
    /// All messages are passed via Arc<T> for zero-copy performance.
    pub fn new() -> Self {
        Self::with_config(TransportConfig::default())
    }

    /// Bind a Unix domain socket endpoint that mirrors all local bus messages.
    ///
    /// External clients (e.g. polygon-terminal) can connect using `UnixTransport::connect`
    /// and receive TLV-framed messages directly from the in-process bus.
    pub async fn bind_unix_endpoint<P: AsRef<std::path::Path>>(
        &self,
        socket_path: P,
    ) -> Result<SocketEndpointHandle> {
        bind_unix_endpoint(socket_path.as_ref().to_path_buf(), self.local.clone()).await
    }

    /// Bind a TCP endpoint (e.g. 127.0.0.1:9091) that mirrors all local bus messages.
    ///
    /// Returns the handle plus the actual bound address (useful when passing port 0).
    pub async fn bind_tcp_endpoint(
        &self,
        addr: SocketAddr,
    ) -> Result<(SocketEndpointHandle, SocketAddr)> {
        bind_tcp_endpoint(addr, self.local.clone()).await
    }

    /// Automatically bind socket endpoints based on topology configuration.
    ///
    /// This reads the `endpoint` field from the node configuration in the topology
    /// and binds the appropriate endpoint (TCP or Unix) if configured.
    ///
    /// Returns `Ok(Some(handle))` if an endpoint was bound, `Ok(None)` if no endpoint
    /// is configured, or an error if binding fails.
    ///
    /// # Examples
    ///
    /// ```toml
    /// [[nodes]]
    /// name = "polygon-adapter"
    /// services = ["polygon-adapter"]
    /// endpoint = { kind = "tcp", addr = "127.0.0.1:9091" }
    /// ```
    ///
    /// ```rust
    /// let topology = Topology::load("topology.toml")?;
    /// let bus = MessageBus::from_topology(topology, "polygon-adapter");
    /// let _handle = bus.bind_configured_endpoints().await?;
    /// ```
    pub async fn bind_configured_endpoints(&self) -> Result<Option<SocketEndpointHandle>> {
        let topology = self.topology.as_ref().ok_or_else(|| {
            TransportError::ServiceNotFound(
                "No topology configured - use bind_tcp_endpoint() or bind_unix_endpoint() manually"
                    .to_string(),
            )
        })?;

        let node_name = self.node_name.as_ref().ok_or_else(|| {
            TransportError::ServiceNotFound("No node name configured".to_string())
        })?;

        // Find this node in the topology
        let node = topology
            .nodes
            .iter()
            .find(|n| &n.name == node_name)
            .ok_or_else(|| {
                TransportError::ServiceNotFound(format!("Node '{}' not found in topology", node_name))
            })?;

        // Check if endpoint is configured
        let endpoint_config = match &node.endpoint {
            Some(cfg) => cfg,
            None => {
                tracing::debug!("Node '{}' has no endpoint configuration", node_name);
                return Ok(None);
            }
        };

        // Bind the appropriate endpoint type
        match endpoint_config.kind {
            EndpointKind::Tcp => {
                let addr_str = endpoint_config.addr.as_ref().ok_or_else(|| {
                    TransportError::ServiceNotFound(format!(
                        "Node '{}' has TCP endpoint but no addr specified",
                        node_name
                    ))
                })?;

                let addr: SocketAddr = addr_str.parse().map_err(|e| {
                    TransportError::ServiceNotFound(format!(
                        "Invalid TCP address '{}' for node '{}': {}",
                        addr_str, node_name, e
                    ))
                })?;

                let (handle, bound_addr) = self.bind_tcp_endpoint(addr).await?;
                tracing::info!(
                    "✓ TCP endpoint listening on {} for node '{}' (configured via topology)",
                    bound_addr,
                    node_name
                );
                Ok(Some(handle))
            }
            EndpointKind::Unix => {
                let socket_path = topology.socket_path(node_name);
                let handle = self.bind_unix_endpoint(&socket_path).await?;
                tracing::info!(
                    "✓ Unix endpoint listening on {} for node '{}' (configured via topology)",
                    socket_path.display(),
                    node_name
                );
                Ok(Some(handle))
            }
        }
    }

    /// Create a message bus with custom configuration
    pub fn with_config(config: TransportConfig) -> Self {
        Self {
            local: LocalTransport::with_config(config.clone()),
            config,
            topology: None,
            node_name: None,
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
    /// This enables hybrid transport: Arc<T> within node, Unix/TCP between nodes.
    pub fn from_topology(topology: Topology, node_name: impl Into<String>) -> Self {
        Self::from_topology_with_config(topology, node_name, TransportConfig::default())
    }

    /// Create a message bus from topology configuration with custom transport config
    pub fn from_topology_with_config(
        topology: Topology,
        node_name: impl Into<String>,
        config: TransportConfig,
    ) -> Self {
        Self {
            local: LocalTransport::with_config(config.clone()),
            config,
            topology: Some(Arc::new(topology)),
            node_name: Some(node_name.into()),
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

    /// Get a publisher for an explicit topic (Phase 1: Actor-ready)
    ///
    /// This enables dynamic topic creation for actor mailboxes and partitioned topics.
    ///
    /// # Example
    /// ```
    /// use crate::MessageBus;
    /// use mycelium_protocol::routing::ActorId;
    ///
    /// let bus = MessageBus::new();
    /// let actor_id = ActorId::from_u64(123);
    /// let topic = format!("actor.{:016x}", actor_id.as_u64());
    /// // let publisher = bus.publisher_for_topic::<MyMessage>(&topic);
    /// ```
    pub fn publisher_for_topic<M: Message>(&self, topic: &str) -> Publisher<M> {
        self.local.publisher_for_topic(topic)
    }

    /// Get a subscriber for an explicit topic (Phase 1: Actor-ready)
    ///
    /// This enables dynamic topic subscription for actor mailboxes and partitioned topics.
    ///
    /// # Example
    /// ```
    /// use crate::MessageBus;
    /// use mycelium_protocol::routing::ActorId;
    ///
    /// let bus = MessageBus::new();
    /// let actor_id = ActorId::from_u64(123);
    /// let topic = format!("actor.{:016x}", actor_id.as_u64());
    /// // let subscriber = bus.subscriber_for_topic::<MyMessage>(&topic);
    /// ```
    pub fn subscriber_for_topic<M: Message>(&self, topic: &str) -> Subscriber<M> {
        self.local.subscriber_for_topic(topic)
    }

    /// Get a Unix publisher to a specific node
    ///
    /// Returns None if no topology is configured or node not found.
    pub async fn unix_publisher<M: Message>(&self, target_node: &str) -> Option<UnixPublisher<M>> {
        let topology = self.topology.as_ref()?;
        let socket_path = topology.socket_path(target_node);

        let transport =
            Self::get_or_create_transport(&self.unix_transports, target_node, || async move {
                UnixTransport::connect(&socket_path).await.ok()
            })
            .await?;

        transport.publisher()
    }

    /// Get a Unix subscriber from a specific node
    ///
    /// Returns None if no topology is configured or node not found.
    pub async fn unix_subscriber<M: Message>(
        &self,
        source_node: &str,
    ) -> Option<UnixSubscriber<M>> {
        let topology = self.topology.as_ref()?;
        let socket_path = topology.socket_path(source_node);

        let transport =
            Self::get_or_create_transport(&self.unix_transports, source_node, || async move {
                UnixTransport::connect(&socket_path).await.ok()
            })
            .await?;

        Some(transport.subscriber())
    }

    /// Get a TCP publisher to a specific remote node
    ///
    /// Returns None if no topology is configured or node address not found.
    pub async fn tcp_publisher<M: Message>(&self, target_node: &str) -> Option<TcpPublisher<M>> {
        let topology = self.topology.as_ref()?;

        // Find node by name and build socket address
        let node = topology.nodes.iter().find(|n| n.name == target_node)?;
        let host = node.host.as_ref()?;
        let port = node.port?;
        let addr: SocketAddr = format!("{}:{}", host, port).parse().ok()?;

        let transport =
            Self::get_or_create_transport(&self.tcp_transports, target_node, || async move {
                TcpTransport::connect(addr).await.ok()
            })
            .await?;

        transport.publisher()
    }

    /// Get a TCP subscriber from a specific remote node
    ///
    /// Returns None if no topology is configured or node address not found.
    pub async fn tcp_subscriber<M: Message>(&self, source_node: &str) -> Option<TcpSubscriber<M>> {
        let topology = self.topology.as_ref()?;

        // Find node by name and build socket address
        let node = topology.nodes.iter().find(|n| n.name == source_node)?;
        let host = node.host.as_ref()?;
        let port = node.port?;
        let addr: SocketAddr = format!("{}:{}", host, port).parse().ok()?;

        let transport =
            Self::get_or_create_transport(&self.tcp_transports, source_node, || async move {
                TcpTransport::connect(addr).await.ok()
            })
            .await?;

        Some(transport.subscriber())
    }

    /// Get a publisher to a specific service with automatic transport selection
    ///
    /// This is the recommended way to get publishers in node-based deployments.
    /// The transport is automatically selected based on topology configuration:
    /// - Same node → Local (Arc<T>)
    /// - Different node, same machine → Unix socket
    /// - Different machine → TCP
    ///
    /// Returns an error if:
    /// - No topology is configured (use `publisher()` for monolith mode)
    /// - Target service not found in topology
    /// - Transport initialization fails
    pub async fn publisher_to<M>(&self, target_service: &str) -> Result<AnyPublisher<M>>
    where
        M: Message + zerocopy::AsBytes,
    {
        let topology = self.topology.as_ref().ok_or_else(|| {
            TransportError::ServiceNotFound(
                "No topology configured - use publisher() for monolith mode".to_string(),
            )
        })?;

        let my_node = self.node_name.as_ref().ok_or_else(|| {
            TransportError::ServiceNotFound("No node name configured".to_string())
        })?;

        let (target_node, transport) =
            Self::resolve_route(topology, my_node, target_service, "target")?;

        match transport {
            TransportType::Local => {
                // Same node - use Arc<T>
                Ok(AnyPublisher::Local(self.local.publisher()))
            }
            TransportType::Unix => {
                // Different node, same machine
                let pub_ = self
                    .unix_publisher(&target_node.name)
                    .await
                    .ok_or_else(|| {
                        TransportError::ServiceNotFound(format!(
                            "Failed to create Unix publisher to '{}'",
                            target_node.name
                        ))
                    })?;
                Ok(AnyPublisher::Unix(pub_))
            }
            TransportType::Tcp => {
                // Different machine
                let pub_ = self.tcp_publisher(&target_node.name).await.ok_or_else(|| {
                    TransportError::ServiceNotFound(format!(
                        "Failed to create TCP publisher to '{}'",
                        target_node.name
                    ))
                })?;
                Ok(AnyPublisher::Tcp(pub_))
            }
        }
    }

    /// Get a subscriber from a specific service with automatic transport selection
    ///
    /// This is the recommended way to get subscribers in node-based deployments.
    /// The transport is automatically selected based on topology configuration:
    /// - Same node → Local (Arc<T>)
    /// - Different node, same machine → Unix socket
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

        let my_node = self.node_name.as_ref().ok_or_else(|| {
            TransportError::ServiceNotFound("No node name configured".to_string())
        })?;

        let (source_node, transport) =
            Self::resolve_route(topology, my_node, source_service, "source")?;

        match transport {
            TransportType::Local => {
                // Same node - use Arc<T>
                Ok(AnySubscriber::Local(self.local.subscriber()))
            }
            TransportType::Unix => {
                // Different node, same machine
                let sub = self
                    .unix_subscriber(&source_node.name)
                    .await
                    .ok_or_else(|| {
                        TransportError::ServiceNotFound(format!(
                            "Failed to create Unix subscriber from '{}'",
                            source_node.name
                        ))
                    })?;
                Ok(AnySubscriber::Unix(sub))
            }
            TransportType::Tcp => {
                // Different machine
                let sub = self
                    .tcp_subscriber(&source_node.name)
                    .await
                    .ok_or_else(|| {
                        TransportError::ServiceNotFound(format!(
                            "Failed to create TCP subscriber from '{}'",
                            source_node.name
                        ))
                    })?;
                Ok(AnySubscriber::Tcp(sub))
            }
        }
    }

    fn resolve_route<'a>(
        topology: &'a Topology,
        my_node: &'a str,
        peer_service: &str,
        peer_role: &'static str,
    ) -> Result<(&'a Node, TransportType)> {
        let peer_node = topology.find_node(peer_service).ok_or_else(|| {
            TransportError::ServiceNotFound(format!(
                "Service '{}' not found in topology",
                peer_service
            ))
        })?;

        let transport = if topology.same_node(my_node, &peer_node.name) {
            TransportType::Local
        } else {
            let my_service = topology
                .nodes
                .iter()
                .find(|n| n.name == my_node)
                .and_then(|n| n.services.first())
                .ok_or_else(|| {
                    TransportError::ServiceNotFound("No services in my node".to_string())
                })?;

            let peer_service_name = peer_node.services.first().ok_or_else(|| {
                TransportError::ServiceNotFound(format!("No services in {} node", peer_role))
            })?;

            topology.transport_between(my_service, peer_service_name)
        };

        Ok((peer_node, transport))
    }

    /// Get the number of active subscribers for a message type
    pub fn subscriber_count<M: Message>(&self) -> usize {
        self.local.subscriber_count::<M>()
    }

    /// Get the node name this bus belongs to
    pub fn node_name(&self) -> Option<&str> {
        self.node_name.as_deref()
    }

    /// Get the topology configuration
    pub fn topology(&self) -> Option<&Topology> {
        self.topology.as_deref()
    }

    /// Create a bounded publisher-subscriber pair (with backpressure)
    ///
    /// Unlike the broadcast-based `publisher`/`subscriber`, this creates a
    /// point-to-point channel with bounded capacity. When the channel is full,
    /// publishers block until the subscriber consumes messages.
    ///
    /// **Use cases**:
    /// - Actor mailboxes (future actor system)
    /// - Flow control to prevent memory exhaustion
    /// - Rate-limiting message streams
    ///
    /// **Tradeoff**: Only supports single subscriber (vs broadcast's fan-out)
    ///
    /// # Example
    ///
    /// ```rust
    /// # use crate::MessageBus;
    /// # use mycelium_protocol::DataEvent;
    /// # async fn example() {
    /// let bus = MessageBus::new();
    ///
    /// // Create bounded channel with capacity 100
    /// let (pub_, mut sub) = bus.bounded_pair::<DataEvent>(100);
    ///
    /// pub_.publish(DataEvent::default()).await.unwrap();
    /// let msg = sub.recv().await.unwrap();
    /// # }
    /// ```
    pub fn bounded_pair<M: Message>(
        &self,
        capacity: usize,
    ) -> (BoundedPublisher<M>, BoundedSubscriber<M>) {
        BoundedPublisher::new(capacity)
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
    use crate::tcp::TcpTransport;
    use crate::unix::UnixTransport;
    use mycelium_protocol::impl_message;
    use zerocopy::{AsBytes, FromBytes, FromZeroes};

    // Generic test message (domain-agnostic)
    #[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
    #[repr(C)]
    struct TestEvent {
        entity_id: u64,
        value: u64,
    }

    impl_message!(TestEvent, 1, "test.events");

    #[tokio::test]
    async fn test_unix_endpoint_broadcasts() {
        let bus = MessageBus::new();
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("bus.sock");

        let handle = bus
            .bind_unix_endpoint(&socket_path)
            .await
            .expect("bind unix endpoint");

        // Allow listener to start
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let client = UnixTransport::connect(&socket_path)
            .await
            .expect("connect to unix endpoint");
        let mut remote_sub = client.subscriber::<TestEvent>();
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let publisher = bus.publisher::<TestEvent>();
        publisher
            .publish(TestEvent {
                entity_id: 42,
                value: 1337,
            })
            .await
            .unwrap();

        let received = tokio::time::timeout(tokio::time::Duration::from_secs(1), remote_sub.recv())
            .await
            .expect("timed out waiting for unix bridge message")
            .expect("unix bridge stream closed");

        assert_eq!(received.value, 1337);

        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_tcp_endpoint_broadcasts() {
        let bus = MessageBus::new();

        let (handle, addr) = bus
            .bind_tcp_endpoint("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind tcp endpoint");

        // Allow listener to start
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let client = TcpTransport::connect(addr)
            .await
            .expect("connect to tcp endpoint");
        let mut remote_sub = client.subscriber::<TestEvent>();
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let publisher = bus.publisher::<TestEvent>();
        publisher
            .publish(TestEvent {
                entity_id: 7,
                value: 9,
            })
            .await
            .unwrap();

        let received = tokio::time::timeout(tokio::time::Duration::from_secs(1), remote_sub.recv())
            .await
            .expect("timed out waiting for tcp bridge message")
            .expect("tcp bridge closed");

        assert_eq!(received.entity_id, 7);
        assert_eq!(received.value, 9);

        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_unix_endpoint_remote_publish_to_local() {
        let bus = MessageBus::new();
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("bus.sock");

        let handle = bus
            .bind_unix_endpoint(&socket_path)
            .await
            .expect("bind unix endpoint");

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let mut local_sub = bus.subscriber::<TestEvent>();

        let client = UnixTransport::connect(&socket_path)
            .await
            .expect("connect remote publisher");
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let publisher = client.publisher::<TestEvent>().expect("remote publisher");
        publisher
            .publish(TestEvent {
                entity_id: 500,
                value: 777,
            })
            .await
            .unwrap();

        let received = tokio::time::timeout(tokio::time::Duration::from_secs(1), local_sub.recv())
            .await
            .expect("timeout waiting for remote publish")
            .expect("local subscriber closed");

        assert_eq!(received.entity_id, 500);
        assert_eq!(received.value, 777);

        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_unix_endpoint_remote_rebroadcast() {
        let bus = MessageBus::new();
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("bus.sock");

        let handle = bus
            .bind_unix_endpoint(&socket_path)
            .await
            .expect("bind unix endpoint");

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Ensure type registry knows about TestEvent even if no local subscribers remain
        drop(bus.publisher::<TestEvent>());

        let publisher_client = UnixTransport::connect(&socket_path)
            .await
            .expect("connect pub client");
        let subscriber_client = UnixTransport::connect(&socket_path)
            .await
            .expect("connect sub client");

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let remote_publisher = publisher_client
            .publisher::<TestEvent>()
            .expect("remote publisher");
        let mut remote_subscriber = subscriber_client.subscriber::<TestEvent>();

        remote_publisher
            .publish(TestEvent {
                entity_id: 321,
                value: 654,
            })
            .await
            .unwrap();

        let received = tokio::time::timeout(
            tokio::time::Duration::from_secs(1),
            remote_subscriber.recv(),
        )
        .await
        .expect("timeout waiting for remote rebroadcast")
        .expect("remote subscriber closed");

        assert_eq!(received.entity_id, 321);
        assert_eq!(received.value, 654);

        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_tcp_endpoint_remote_publish_to_local() {
        let bus = MessageBus::new();

        let (handle, addr) = bus
            .bind_tcp_endpoint("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind tcp endpoint");

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let mut local_sub = bus.subscriber::<TestEvent>();

        let client = TcpTransport::connect(addr)
            .await
            .expect("connect remote publisher");
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let publisher = client.publisher::<TestEvent>().expect("remote publisher");
        publisher
            .publish(TestEvent {
                entity_id: 999,
                value: 1,
            })
            .await
            .unwrap();

        let received = tokio::time::timeout(tokio::time::Duration::from_secs(1), local_sub.recv())
            .await
            .expect("timeout waiting for tcp remote publish")
            .expect("local subscriber closed");

        assert_eq!(received.entity_id, 999);
        assert_eq!(received.value, 1);

        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_message_bus_basic() {
        let bus = MessageBus::new();

        let pub_ = bus.publisher::<TestEvent>();
        let mut sub = bus.subscriber::<TestEvent>();

        pub_.publish(TestEvent {
            entity_id: 1,
            value: 1000,
        })
        .await
        .unwrap();

        let event = sub.recv().await.unwrap();
        assert_eq!(event.entity_id, 1);
        assert_eq!(event.value, 1000);
    }

    #[tokio::test]
    async fn test_multiple_publishers() {
        let bus = MessageBus::new();

        let pub1 = bus.publisher::<TestEvent>();
        let pub2 = bus.publisher::<TestEvent>();
        let mut sub = bus.subscriber::<TestEvent>();

        pub1.publish(TestEvent {
            entity_id: 1,
            value: 1000,
        })
        .await
        .unwrap();

        pub2.publish(TestEvent {
            entity_id: 2,
            value: 2000,
        })
        .await
        .unwrap();

        let event1 = sub.recv().await.unwrap();
        let event2 = sub.recv().await.unwrap();

        assert_eq!(event1.entity_id, 1);
        assert_eq!(event2.entity_id, 2);
    }

    #[tokio::test]
    async fn test_subscriber_count() {
        let bus = MessageBus::new();

        let _sub1 = bus.subscriber::<TestEvent>();
        let _sub2 = bus.subscriber::<TestEvent>();

        assert_eq!(bus.subscriber_count::<TestEvent>(), 2);
    }

    #[tokio::test]
    async fn test_custom_capacity() {
        let bus = MessageBus::with_capacity(5);

        let pub_ = bus.publisher::<TestEvent>();
        let mut sub = bus.subscriber::<TestEvent>();

        pub_.publish(TestEvent {
            entity_id: 1,
            value: 1000,
        })
        .await
        .unwrap();

        let event = sub.recv().await.unwrap();
        assert_eq!(event.entity_id, 1);
    }

    #[tokio::test]
    async fn test_bundled_deployment() {
        use crate::config::{Node, Topology};

        // Create topology with 2 nodes
        let dir = tempfile::tempdir().unwrap();
        let topology = Topology {
            nodes: vec![
                Node {
                    name: "collectors".to_string(),
                    services: vec!["data-collector".to_string()],
                    host: None,
                    port: None,
                },
                Node {
                    name: "processors".to_string(),
                    services: vec!["processor".to_string()],
                    host: None,
                    port: None,
                },
            ],
            socket_dir: dir.path().to_path_buf(),
        };

        // Node 1: collectors (server side - binds socket)
        let socket_path = topology.socket_path("collectors");
        let _adapter_transport = crate::unix::UnixTransport::bind(&socket_path)
            .await
            .unwrap();

        // Give server time to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Node 1: Create message bus for collectors
        let collectors_bus = MessageBus::from_topology(topology.clone(), "collectors");

        // Node 1: Create subscriber first (so publish doesn't fail)
        let _sub = collectors_bus.subscriber::<TestEvent>();

        // Node 1: Publish locally and listen for external subscriptions
        let adapter_pub = collectors_bus.publisher::<TestEvent>();
        adapter_pub
            .publish(TestEvent {
                entity_id: 100,
                value: 5000,
            })
            .await
            .unwrap();

        // Node 2: Create message bus for processors
        let processors_bus = MessageBus::from_topology(topology, "processors");

        // Node 2: Get Unix publisher to send to collectors node
        let unix_pub = processors_bus
            .unix_publisher::<TestEvent>("collectors")
            .await
            .expect("Failed to create Unix publisher");

        // Node 2: Publish message to collectors node
        unix_pub
            .publish(TestEvent {
                entity_id: 200,
                value: 10000,
            })
            .await
            .unwrap();

        // Verify metadata
        assert_eq!(collectors_bus.node_name(), Some("collectors"));
        assert_eq!(processors_bus.node_name(), Some("processors"));
        assert!(collectors_bus.topology().is_some());
    }

    #[tokio::test]
    async fn test_from_topology() {
        use crate::config::Topology;

        let dir = tempfile::tempdir().unwrap();
        let topology = Topology {
            nodes: vec![],
            socket_dir: dir.path().to_path_buf(),
        };

        let bus = MessageBus::from_topology(topology, "test-node");

        assert_eq!(bus.node_name(), Some("test-node"));
        assert!(bus.topology().is_some());
    }

    #[tokio::test]
    async fn test_distributed_deployment() {
        use crate::config::{Node, Topology};

        // Create topology with 2 nodes on different hosts
        let dir = tempfile::tempdir().unwrap();
        let topology = Topology {
            nodes: vec![
                Node {
                    name: "collectors".to_string(),
                    services: vec!["data-collector".to_string()],
                    host: Some("127.0.0.1".to_string()),
                    port: Some(0), // Will be replaced with actual port
                },
                Node {
                    name: "processors".to_string(),
                    services: vec!["processor".to_string()],
                    host: Some("127.0.0.1".to_string()),
                    port: Some(0),
                },
            ],
            socket_dir: dir.path().to_path_buf(),
        };

        // Node 1: collectors (server side - binds socket)
        let adapter_addr = "127.0.0.1:0".parse().unwrap();
        let adapter_transport = crate::tcp::TcpTransport::bind(adapter_addr).await.unwrap();
        let adapter_bind_addr = adapter_transport.local_addr();

        // Update topology with actual port
        let mut updated_topology = topology.clone();
        updated_topology.nodes[0].port = Some(adapter_bind_addr.port());

        // Give server time to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Node 1: Create message bus for collectors
        let collectors_bus = MessageBus::from_topology(updated_topology.clone(), "collectors");

        // Node 1: Create subscriber first (so publish doesn't fail)
        let _sub = collectors_bus.subscriber::<TestEvent>();

        // Node 1: Publish locally
        let adapter_pub = collectors_bus.publisher::<TestEvent>();
        adapter_pub
            .publish(TestEvent {
                entity_id: 100,
                value: 5000,
            })
            .await
            .unwrap();

        // Node 2: Create message bus for processors
        let processors_bus = MessageBus::from_topology(updated_topology, "processors");

        // Node 2: Get TCP publisher to send to collectors node
        let tcp_pub = processors_bus
            .tcp_publisher::<TestEvent>("collectors")
            .await
            .expect("Failed to create TCP publisher");

        // Node 2: Publish message to collectors node
        tcp_pub
            .publish(TestEvent {
                entity_id: 200,
                value: 10000,
            })
            .await
            .unwrap();

        // Verify metadata
        assert_eq!(collectors_bus.node_name(), Some("collectors"));
        assert_eq!(processors_bus.node_name(), Some("processors"));
        assert!(collectors_bus.topology().is_some());
    }

    #[tokio::test]
    async fn test_publisher_to_same_node() {
        use crate::config::{Node, Topology};

        // Create topology with services in same node
        let dir = tempfile::tempdir().unwrap();
        let topology = Topology {
            nodes: vec![Node {
                name: "trading".to_string(),
                services: vec!["state-manager".to_string(), "executor".to_string()],
                host: None,
                port: None,
            }],
            socket_dir: dir.path().to_path_buf(),
        };

        let bus = MessageBus::from_topology(topology, "trading");

        // Get publisher to service in same node
        let pub_ = bus.publisher_to::<TestEvent>("executor").await.unwrap();

        // Should use local transport
        assert_eq!(pub_.transport_type(), "local");

        // Verify it works
        let mut sub = bus.subscriber::<TestEvent>();
        pub_.publish(TestEvent {
            entity_id: 1,
            value: 100,
        })
        .await
        .unwrap();

        let event = sub.recv().await.unwrap();
        assert_eq!(event.entity_id, 1);
    }

    #[tokio::test]
    async fn test_publisher_to_different_node_unix() {
        use crate::config::{Node, Topology};

        let dir = tempfile::tempdir().unwrap();
        let topology = Topology {
            nodes: vec![
                Node {
                    name: "manager".to_string(),
                    services: vec!["state-manager".to_string()],
                    host: None,
                    port: None,
                },
                Node {
                    name: "executor".to_string(),
                    services: vec!["executor".to_string()],
                    host: None,
                    port: None,
                },
            ],
            socket_dir: dir.path().to_path_buf(),
        };

        // Bind server for executor node
        let executor_socket = topology.socket_path("executor");
        let _executor_server = crate::unix::UnixTransport::bind(&executor_socket)
            .await
            .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let manager_bus = MessageBus::from_topology(topology, "manager");

        // Get publisher to different node
        let pub_ = manager_bus
            .publisher_to::<TestEvent>("executor")
            .await
            .unwrap();

        // Should use Unix transport
        assert_eq!(pub_.transport_type(), "unix");
    }

    #[tokio::test]
    async fn test_publisher_to_distributed_tcp() {
        use crate::config::{Node, Topology};

        // Bind a real TCP server for executor node
        let executor_server = crate::tcp::TcpTransport::bind("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let executor_addr = executor_server.local_addr();

        let dir = tempfile::tempdir().unwrap();
        let topology = Topology {
            nodes: vec![
                Node {
                    name: "manager".to_string(),
                    services: vec!["state-manager".to_string()],
                    host: Some("192.168.1.10".to_string()), // Different host
                    port: Some(9001),
                },
                Node {
                    name: "executor".to_string(),
                    services: vec!["executor".to_string()],
                    host: Some(executor_addr.ip().to_string()), // 127.0.0.1
                    port: Some(executor_addr.port()),
                },
            ],
            socket_dir: dir.path().to_path_buf(),
        };

        let manager_bus = MessageBus::from_topology(topology, "manager");

        // Get publisher to different host
        let pub_ = manager_bus
            .publisher_to::<TestEvent>("executor")
            .await
            .unwrap();

        // Should use TCP transport
        assert_eq!(pub_.transport_type(), "tcp");
    }

    #[tokio::test]
    async fn test_subscriber_from_same_node() {
        use crate::config::{Node, Topology};

        let dir = tempfile::tempdir().unwrap();
        let topology = Topology {
            nodes: vec![Node {
                name: "trading".to_string(),
                services: vec!["state-manager".to_string(), "executor".to_string()],
                host: None,
                port: None,
            }],
            socket_dir: dir.path().to_path_buf(),
        };

        let bus = MessageBus::from_topology(topology, "trading");

        // Get subscriber from service in same node
        let mut sub = bus.subscriber_from::<TestEvent>("executor").await.unwrap();

        // Should use local transport
        assert_eq!(sub.transport_type(), "local");

        // Verify it works
        let pub_ = bus.publisher::<TestEvent>();
        pub_.publish(TestEvent {
            entity_id: 2,
            value: 200,
        })
        .await
        .unwrap();

        let event = sub.recv().await.unwrap();
        assert_eq!(event.entity_id, 2);
    }

    #[tokio::test]
    async fn test_smart_routing_error_no_topology() {
        let bus = MessageBus::new();

        // Should fail with no topology
        let result = bus.publisher_to::<TestEvent>("some-service").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_smart_routing_error_service_not_found() {
        use crate::config::{Node, Topology};

        let dir = tempfile::tempdir().unwrap();
        let topology = Topology {
            nodes: vec![Node {
                name: "trading".to_string(),
                services: vec!["state-manager".to_string()],
                host: None,
                port: None,
            }],
            socket_dir: dir.path().to_path_buf(),
        };

        let bus = MessageBus::from_topology(topology, "trading");

        // Should fail with service not found
        let result = bus.publisher_to::<TestEvent>("nonexistent-service").await;
        assert!(result.is_err());
    }
}
