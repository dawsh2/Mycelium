//! Distributed deployment integration tests
//!
//! Services across different machines using TCP transport.

use crate::integration::{ArbitrageSignal, MessageBus, SwapEvent};
use mycelium_config::{Node, Topology};
use mycelium_transport::TcpTransport;
use std::net::SocketAddr;
use tokio::time::sleep;

#[tokio::test]
async fn test_tcp_transport_between_hosts() {
    let topology = create_distributed_topology();

    // Start TCP transport for remote bundle
    let remote_addr = "127.0.0.1:0".parse().unwrap();
    let remote_transport = TcpTransport::bind(remote_addr).await.unwrap();
    let remote_bind_addr = remote_transport.local_addr();

    // Update topology with actual port
    let mut updated_topology = topology.clone();
    updated_topology.nodes[1].port = Some(remote_bind_addr.port());

    sleep(tokio::time::Duration::from_millis(100)).await;

    let local_bus = MessageBus::from_topology(updated_topology.clone(), "adapters");
    let remote_bus = MessageBus::from_topology(updated_topology, "strategies");

    // Test TCP publisher to remote
    let tcp_publisher = local_bus
        .tcp_publisher::<SwapEvent>("strategies")
        .await
        .expect("Failed to create TCP publisher");

    assert_eq!(tcp_publisher.transport_type(), "tcp");

    // Test message flow
    let mut remote_subscriber = remote_bus.subscriber::<SwapEvent>();
    let event = SwapEvent {
        pool_id: 600,
        amount_in: 6000,
        amount_out: 12000,
        timestamp: 22222,
    };

    tcp_publisher.publish(event).await.unwrap();

    let received = remote_subscriber.recv().await.unwrap();
    assert_eq!(*received, event);
}

#[tokio::test]
async fn test_smart_routing_uses_tcp_for_different_hosts() {
    let topology = create_distributed_topology();

    // Start TCP transport for remote
    let remote_addr = "127.0.0.1:0".parse().unwrap();
    let _remote_transport = TcpTransport::bind(remote_addr).await.unwrap();

    let local_bus = MessageBus::from_topology(topology, "adapters");

    // Should use TCP for different hosts
    let publisher = local_bus
        .publisher_to::<SwapEvent>("flash-arbitrage")
        .await
        .expect("Failed to create publisher");

    assert_eq!(publisher.transport_type(), "tcp");
}

#[tokio::test]
async fn test_tcp_connection_failure_handling() {
    let topology = create_distributed_topology();

    // Don't start server - this should fail
    let local_bus = MessageBus::from_topology(topology, "adapters");

    let result = local_bus.tcp_publisher::<SwapEvent>("strategies").await;
    assert!(result.is_none());
}

#[tokio::test]
async fn test_high_latency_tcp_messages() {
    let topology = create_distributed_topology();

    // Start TCP transport for remote
    let remote_addr = "127.0.0.1:0".parse().unwrap();
    let _remote_transport = TcpTransport::bind(remote_addr).await.unwrap();

    let local_bus = MessageBus::from_topology(topology.clone(), "adapters");
    let remote_bus = MessageBus::from_topology(topology, "strategies");

    let publisher = local_bus
        .tcp_publisher::<SwapEvent>("strategies")
        .await
        .expect("Failed to create TCP publisher");

    let mut subscriber = remote_bus.subscriber::<SwapEvent>();

    let message_count = 100;
    let start_time = std::time::Instant::now();

    for i in 0..message_count {
        publisher.publish(SwapEvent {
            pool_id: i,
            amount_in: i as u128 * 1000,
            amount_out: i as u128 * 2000,
            timestamp: i as u64,
        }).await.unwrap();
    }

    for i in 0..message_count {
        let received = subscriber.recv().await.unwrap();
        assert_eq!(received.pool_id, i);
    }

    let elapsed = start_time.elapsed();
    println!("Sent {} messages via TCP in {:?}", message_count, elapsed);

    // TCP should be slower than Arc but still reasonable
    assert!(elapsed.as_millis() < 1000); // Less than 1 second for 100 messages
}

fn create_distributed_topology() -> Topology {
    Topology {
        nodes: vec![
            Node {
                name: "adapters".to_string(),
                services: vec!["polygon-adapter".to_string()],
                host: Some("127.0.0.1".to_string()),
                port: Some(9001),
            },
            Node {
                name: "strategies".to_string(),
                services: vec!["flash-arbitrage".to_string()],
                host: Some("192.168.1.100".to_string()), // Different host
                port: Some(0), // Will be replaced with actual port
            },
        ],
        socket_dir: std::path::PathBuf::from("/tmp/mycelium"),
    }
}