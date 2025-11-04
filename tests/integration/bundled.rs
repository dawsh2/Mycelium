//! Bundled deployment integration tests
//!
//! Services grouped in nodes with Arc<T> within nodes and Unix/TCP between nodes.

use crate::integration::{ArbitrageSignal, MessageBus, OrderExecution, SwapEvent};
use mycelium_transport::config::{Node, Topology};
use mycelium_transport::{UnixTransport, TcpTransport};
use tempfile::TempDir;
use tokio::time::sleep;

#[tokio::test]
async fn test_unix_transport_between_nodes() {
    let temp_dir = TempDir::new().unwrap();
    let socket_dir = temp_dir.path();

    let topology = create_bundled_topology(socket_dir);

    // Node 1: adapters (server side)
    let socket_path = topology.socket_path("adapters");
    let _server = UnixTransport::bind(&socket_path).await.unwrap();

    // Give server time to start
    sleep(tokio::time::Duration::from_millis(100)).await;

    // Node 1: Create message bus
    let adapters_bus = MessageBus::from_topology(topology.clone(), "adapters");
    let local_pub = adapters_bus.publisher::<SwapEvent>();
    let mut local_sub = adapters_bus.subscriber::<ArbitrageSignal>();

    // Node 2: Create message bus for strategies
    let strategies_bus = MessageBus::from_topology(topology, "strategies");

    // Test Unix publisher to adapters
    let unix_pub = strategies_bus
        .unix_publisher::<SwapEvent>("adapters")
        .await
        .expect("Failed to create Unix publisher");

    let mut unix_sub = strategies_bus
        .unix_subscriber::<ArbitrageSignal>("strategies")
        .await
        .expect("Failed to create Unix subscriber");

    // Test messages
    let swap_event = SwapEvent {
        pool_id: 100,
        amount_in: 1000,
        amount_out: 2000,
        timestamp: 12345,
    };

    let arb_signal = ArbitrageSignal {
        opportunity_id: 200,
        profit: 5000,
        timestamp: 12346,
    };

    // Node 2 sends to Node 1 via Unix
    unix_pub.publish(swap_event).await.unwrap();

    // Node 1 should receive via Unix
    sleep(tokio::time::Duration::from_millis(50)).await;

    // Node 1 sends to Node 2 via Unix
    local_pub.publish(arb_signal).await.unwrap();

    // Node 2 should receive via Unix
    let received_signal = unix_sub.recv().await.unwrap();
    assert_eq!(received_signal.opportunity_id, 200);
}

#[tokio::test]
async fn test_local_transport_within_node() {
    let temp_dir = TempDir::new().unwrap();
    let topology = create_bundled_topology(temp_dir.path());

    let adapters_bus = MessageBus::from_topology(topology, "adapters");

    let publisher = adapters_bus.publisher::<SwapEvent>();
    let mut subscriber = adapters_bus.subscriber::<SwapEvent>();

    let event = SwapEvent {
        pool_id: 300,
        amount_in: 3000,
        amount_out: 6000,
        timestamp: 54321,
    };

    publisher.publish(event).await.unwrap();
    let received = subscriber.recv().await.unwrap();

    assert_eq!(*received, event);
}

#[tokio::test]
async fn test_smart_routing_within_node() {
    let temp_dir = TempDir::new().unwrap();
    let topology = create_bundled_topology(temp_dir.path());

    let adapters_bus = MessageBus::from_topology(topology, "adapters");

    // publisher_to should use Local transport for same node
    let publisher = adapters_bus
        .publisher_to::<SwapEvent>("polygon-adapter")
        .await
        .expect("Failed to create publisher");

    assert_eq!(publisher.transport_type(), "local");

    // Test it works
    let mut subscriber = adapters_bus.subscriber::<SwapEvent>();
    let event = SwapEvent {
        pool_id: 400,
        amount_in: 4000,
        amount_out: 8000,
        timestamp: 98765,
    };

    publisher.publish(event).await.unwrap();
    let received = subscriber.recv().await.unwrap();

    assert_eq!(*received, event);
}

#[tokio::test]
async fn test_smart_routing_between_nodes() {
    let temp_dir = TempDir::new().unwrap();
    let socket_dir = temp_dir.path();
    let topology = create_bundled_topology(socket_dir);

    // Start Unix transport for strategies node
    let strategies_socket = topology.socket_path("strategies");
    let _strategies_server = UnixTransport::bind(&strategies_socket).await.unwrap();

    sleep(tokio::time::Duration::from_millis(100)).await;

    let adapters_bus = MessageBus::from_topology(topology.clone(), "adapters");
    let strategies_bus = MessageBus::from_topology(topology, "strategies");

    // publisher_to should use Unix transport for different nodes
    let publisher = adapters_bus
        .publisher_to::<SwapEvent>("flash-arbitrage")
        .await
        .expect("Failed to create publisher");

    assert_eq!(publisher.transport_type(), "unix");

    // Test message flow
    let mut subscriber = strategies_bus.subscriber::<SwapEvent>();
    let event = SwapEvent {
        pool_id: 500,
        amount_in: 5000,
        amount_out: 10000,
        timestamp: 11111,
    };

    publisher.publish(event).await.unwrap();

    let received = subscriber.recv().await.unwrap();
    assert_eq!(*received, event);
}

#[tokio::test]
async fn test_node_error_handling() {
    let temp_dir = TempDir::new().unwrap();
    let topology = create_bundled_topology(temp_dir.path());

    let adapters_bus = MessageBus::from_topology(topology, "adapters");

    // Try to publish to non-existent service
    let result = adapters_bus.publisher_to::<SwapEvent>("nonexistent-service").await;
    assert!(result.is_err());

    // Try to publish to non-existent node
    let result = adapters_bus.unix_publisher::<SwapEvent>("nonexistent-node").await;
    assert!(result.is_none());

    // Try to subscribe from non-existent service
    let result = adapters_bus.subscriber_from::<SwapEvent>("nonexistent-service").await;
    assert!(result.is_err());
}

fn create_bundled_topology(socket_dir: &std::path::Path) -> Topology {
    Topology {
        nodes: vec![
            Node {
                name: "adapters".to_string(),
                services: vec!["polygon-adapter".to_string(), "ethereum-adapter".to_string()],
                host: None,
                port: None,
            },
            Node {
                name: "strategies".to_string(),
                services: vec!["flash-arbitrage".to_string()],
                host: None,
                port: None,
            },
        ],
        socket_dir: socket_dir.to_path_buf(),
    }
}