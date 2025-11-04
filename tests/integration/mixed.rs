//! Mixed deployment integration tests
//!
//! Tests combining different transport types and deployment modes.

use crate::integration::{ArbitrageSignal, MessageBus, OrderExecution, SwapEvent};
use mycelium_transport::config::{Node, Topology};
use mycelium_transport::{TcpTransport, UnixTransport};
use tempfile::TempDir;

#[tokio::test]
async fn test_mixed_transport_types() {
    let temp_dir = TempDir::new().unwrap();
    let socket_dir = temp_dir.path();

    let topology = create_mixed_topology(socket_dir);

    // Start Unix transport for local strategies
    let strategies_socket = topology.socket_path("strategies");
    let _strategies_server = UnixTransport::bind(&strategies_socket).await.unwrap();

    // Start TCP transport for remote execution
    let remote_addr = "127.0.0.1:0".parse().unwrap();
    let remote_transport = TcpTransport::bind(remote_addr).await.unwrap();
    let remote_bind_addr = remote_transport.local_addr();

    // Update topology with actual port
    let mut updated_topology = topology.clone();
    updated_topology.nodes[2].port = Some(remote_bind_addr.port());

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create message buses for each node
    let adapters_bus = MessageBus::from_topology(updated_topology.clone(), "adapters");
    let strategies_bus = MessageBus::from_topology(updated_topology.clone(), "strategies");
    let execution_bus = MessageBus::from_topology(updated_topology, "execution");

    // Test local communication within adapters bundle (Arc)
    let local_pub = adapters_bus.publisher::<SwapEvent>();
    let mut local_sub = adapters_bus.subscriber::<ArbitrageSignal>();

    // Test Unix communication with strategies bundle
    let unix_pub = adapters_bus
        .unix_publisher::<SwapEvent>("strategies")
        .await
        .expect("Failed to create Unix publisher");

    let mut unix_sub = adapters_bus
        .unix_subscriber::<ArbitrageSignal>("adapters")
        .await
        .expect("Failed to create Unix subscriber");

    // Test TCP communication with execution bundle
    let tcp_pub = adapters_bus
        .tcp_publisher::<OrderExecution>("execution")
        .await
        .expect("Failed to create TCP publisher");

    let mut tcp_sub = execution_bus.subscriber::<OrderExecution>();

    // Send messages via all transport types
    let swap_event = SwapEvent {
        pool_id: 100,
        amount_in: 1000,
        amount_out: 2000,
        timestamp: 11111,
    };

    let arb_signal = ArbitrageSignal {
        opportunity_id: 200,
        profit: 5000,
        timestamp: 22222,
    };

    let order_exec = OrderExecution {
        order_id: 300,
        success: 1,  // 1 = true
        timestamp: 33333,
    };

    // Local Arc transport
    local_pub.publish(arb_signal).await.unwrap();

    // Unix transport
    unix_pub.publish(swap_event).await.unwrap();

    // TCP transport
    tcp_pub.publish(order_exec).await.unwrap();

    // Verify all messages are received
    let received_local = local_sub.recv().await.unwrap();
    assert_eq!(received_local.opportunity_id, 200);

    let received_unix = strategies_bus.subscriber::<SwapEvent>().recv().await.unwrap();
    assert_eq!(received_unix.pool_id, 100);

    let received_tcp = tcp_sub.recv().await.unwrap();
    assert_eq!(received_tcp.order_id, 300);
}

#[tokio::test]
async fn test_smart_routing_mixed_transports() {
    let temp_dir = TempDir::new().unwrap();
    let socket_dir = temp_dir.path();

    let topology = create_mixed_topology(socket_dir);

    // Start Unix transport for strategies
    let strategies_socket = topology.socket_path("strategies");
    let _strategies_server = UnixTransport::bind(&strategies_socket).await.unwrap();

    // Start TCP transport for execution
    let remote_addr = "127.0.0.1:0".parse().unwrap();
    let remote_transport = TcpTransport::bind(remote_addr).await.unwrap();
    let remote_bind_addr = remote_transport.local_addr();

    // Update topology with actual port
    let mut updated_topology = topology;
    updated_topology.nodes[2].port = Some(remote_bind_addr.port());

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let adapters_bus = MessageBus::from_topology(updated_topology, "adapters");

    // Test routing to same node (Local)
    let same_node_pub = adapters_bus
        .publisher_to::<SwapEvent>("ethereum-adapter")
        .await
        .expect("Failed to create publisher");
    assert_eq!(same_node_pub.transport_type(), "local");

    // Test routing to different node, same host (Unix)
    let unix_pub = adapters_bus
        .publisher_to::<SwapEvent>("flash-arbitrage")
        .await
        .expect("Failed to create publisher");
    assert_eq!(unix_pub.transport_type(), "unix");

    // Test routing to different host (TCP)
    let tcp_pub = adapters_bus
        .publisher_to::<SwapEvent>("order-manager")
        .await
        .expect("Failed to create publisher");
    assert_eq!(tcp_pub.transport_type(), "tcp");
}

#[tokio::test]
async fn test_transport_fallback_behavior() {
    let temp_dir = TempDir::new().unwrap();
    let socket_dir = temp_dir.path();

    let topology = Topology {
        nodes: vec![
            Node {
                name: "adapters".to_string(),
                services: vec!["polygon-adapter".to_string()],
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
    };

    let adapters_bus = MessageBus::from_topology(topology, "adapters");

    // Without server running, Unix connection should fail
    let result = adapters_bus.unix_publisher::<SwapEvent>("strategies").await;
    assert!(result.is_none());

    // But local transport should still work
    let local_pub = adapters_bus.publisher::<SwapEvent>();
    assert!(local_pub.publish(SwapEvent {
        pool_id: 1,
        amount_in: 100,
        amount_out: 200,
        timestamp: 12345,
    }).await.is_ok());
}

#[tokio::test]
async fn test_concurrent_messaging_across_transports() {
    let temp_dir = TempDir::new().unwrap();
    let socket_dir = temp_dir.path();

    let topology = create_mixed_topology(socket_dir);

    // Start Unix transport for strategies
    let strategies_socket = topology.socket_path("strategies");
    let _strategies_server = UnixTransport::bind(&strategies_socket).await.unwrap();

    // Start TCP transport for execution
    let remote_addr = "127.0.0.1:0".parse().unwrap();
    let remote_transport = TcpTransport::bind(remote_addr).await.unwrap();
    let remote_bind_addr = remote_transport.local_addr();

    // Update topology with actual port
    let mut updated_topology = topology;
    updated_topology.nodes[2].port = Some(remote_bind_addr.port());

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let adapters_bus = MessageBus::from_topology(updated_topology, "adapters");
    let strategies_bus = MessageBus::from_topology(updated_topology.clone(), "strategies");
    let execution_bus = MessageBus::from_topology(updated_topology, "execution");

    // Create publishers for all transport types
    let local_pub = adapters_bus.publisher::<SwapEvent>();
    let unix_pub = adapters_bus
        .unix_publisher::<SwapEvent>("strategies")
        .await
        .expect("Failed to create Unix publisher");
    let tcp_pub = adapters_bus
        .tcp_publisher::<SwapEvent>("execution")
        .await
        .expect("Failed to create TCP publisher");

    // Create subscribers
    let mut local_sub = adapters_bus.subscriber::<SwapEvent>();
    let mut unix_sub = strategies_bus.subscriber::<SwapEvent>();
    let mut tcp_sub = execution_bus.subscriber::<SwapEvent>();

    // Send messages concurrently
    let message_count = 50;

    // Spawn concurrent publishers
    let local_handle = tokio::spawn({
        let pub_ = local_pub;
        async move {
            for i in 0..message_count {
                pub_.publish(SwapEvent {
                    pool_id: i,
                    amount_in: i as u128 * 1000,
                    amount_out: i as u128 * 2000,
                    timestamp: i as u64,
                }).await.unwrap();
            }
        }
    });

    let unix_handle = tokio::spawn({
        let pub_ = unix_pub;
        async move {
            for i in 0..message_count {
                pub_.publish(SwapEvent {
                    pool_id: i + 1000,
                    amount_in: (i + 1000) as u128 * 1000,
                    amount_out: (i + 1000) as u128 * 2000,
                    timestamp: (i + 1000) as u64,
                }).await.unwrap();
            }
        }
    });

    let tcp_handle = tokio::spawn({
        let pub_ = tcp_pub;
        async move {
            for i in 0..message_count {
                pub_.publish(SwapEvent {
                    pool_id: i + 2000,
                    amount_in: (i + 2000) as u128 * 1000,
                    amount_out: (i + 2000) as u128 * 2000,
                    timestamp: (i + 2000) as u64,
                }).await.unwrap();
            }
        }
    });

    // Wait for all publishers to complete
    local_handle.await.unwrap();
    unix_handle.await.unwrap();
    tcp_handle.await.unwrap();

    // Verify all messages are received
    for i in 0..message_count {
        let local_msg = local_sub.recv().await.unwrap();
        assert_eq!(local_msg.pool_id, i);

        let unix_msg = unix_sub.recv().await.unwrap();
        assert_eq!(unix_msg.pool_id, i + 1000);

        let tcp_msg = tcp_sub.recv().await.unwrap();
        assert_eq!(tcp_msg.pool_id, i + 2000);
    }
}

fn create_mixed_topology(socket_dir: &std::path::Path) -> Topology {
    Topology {
        nodes: vec![
            Node {
                name: "adapters".to_string(),
                services: vec!["polygon-adapter".to_string(), "ethereum-adapter".to_string()],
                host: Some("127.0.0.1".to_string()), // Local host
                port: Some(9001),
            },
            Node {
                name: "strategies".to_string(),
                services: vec!["flash-arbitrage".to_string()],
                host: Some("127.0.0.1".to_string()), // Same host - should use Unix
                port: Some(0), // Will be set dynamically
            },
            Node {
                name: "execution".to_string(),
                services: vec!["order-manager".to_string()],
                host: Some("192.168.1.100".to_string()), // Different host - should use TCP
                port: Some(0), // Will be set dynamically
            },
        ],
        socket_dir: socket_dir.to_path_buf(),
    }
}