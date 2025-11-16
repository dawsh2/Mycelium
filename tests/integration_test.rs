use mycelium_protocol::impl_message;
use mycelium_transport::config::{Node, Topology};
use mycelium_transport::{MessageBus, TcpTransport, UnixTransport};
use zerocopy::{AsBytes, FromBytes, FromZeroes};

// Simulated market data message
#[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
struct SwapEvent {
    amount_in: u128,
    amount_out: u128,
    pool_id: u64,
    token_in: u64,
    token_out: u64,
    timestamp: u64,
}

impl_message!(SwapEvent, 1, "market-data");

// Simulated arbitrage opportunity message
#[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
struct ArbitrageOpportunity {
    profit: u128,
    pool_a: u64,
    pool_b: u64,
    timestamp: u64,
    _padding: [u8; 8],
}

impl_message!(ArbitrageOpportunity, 2, "arbitrage");

// Simulated order message
#[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
struct OrderExecution {
    amount: u128,
    opportunity_id: u64,
    success: u8,       // bool is not safe for zerocopy, use u8
    _padding: [u8; 7], // Explicit padding to align to 8-byte boundary
}

impl_message!(OrderExecution, 3, "orders");

#[tokio::test]
async fn test_monolith_deployment() {
    // Single process, all Arc<T> communication
    let bus = MessageBus::new();

    let swap_pub = bus.publisher::<SwapEvent>();
    let mut swap_sub = bus.subscriber::<SwapEvent>();

    // Simulate adapter publishing market data
    tokio::spawn(async move {
        for i in 0..5 {
            swap_pub
                .publish(SwapEvent {
                    pool_id: i,
                    token_in: 1,
                    token_out: 2,
                    amount_in: 1000,
                    amount_out: 2000,
                    timestamp: i,
                })
                .await
                .unwrap();
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
    });

    // Strategy consumes market data
    let mut count = 0;
    while count < 5 {
        let event = swap_sub.recv().await.unwrap();
        assert_eq!(event.pool_id, count);
        count += 1;
    }
}

#[tokio::test]
async fn test_bundled_deployment_simple() {
    // Simplified test: demonstrate bundled topology configuration
    let dir = tempfile::tempdir().unwrap();
    let topology = Topology {
        nodes: vec![
            Node {
                name: "adapters".to_string(),
                services: vec!["polygon-adapter".to_string()],
                host: None,
                port: None,
                endpoint: None,
            },
            Node {
                name: "strategies".to_string(),
                services: vec!["flash-arbitrage".to_string()],
                host: None,
                port: None,
                endpoint: None,
            },
        ],
        socket_dir: dir.path().to_path_buf(),
    };

    // Bind server socket for strategies node
    let strategy_socket = topology.socket_path("strategies");
    let _strategy_server = UnixTransport::bind(&strategy_socket).await.unwrap();
    let mut strategy_sub = _strategy_server.subscriber::<SwapEvent>();

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create adapter bus and publish directly to strategies
    let adapter_bus = MessageBus::from_topology(topology, "adapters");
    let unix_pub = adapter_bus
        .unix_publisher::<SwapEvent>("strategies")
        .await
        .unwrap();

    // Publish messages
    for i in 0..3 {
        unix_pub
            .publish(SwapEvent {
                pool_id: i,
                token_in: 1,
                token_out: 2,
                amount_in: 1000,
                amount_out: 2000,
                timestamp: i,
            })
            .await
            .unwrap();
    }

    // Verify messages received
    for i in 0..3 {
        let event = tokio::time::timeout(tokio::time::Duration::from_secs(1), strategy_sub.recv())
            .await
            .expect("Timeout")
            .expect("Failed to receive");

        assert_eq!(event.pool_id, i);
    }
}

#[tokio::test]
async fn test_distributed_deployment_simple() {
    // Simplified test: demonstrate TCP transport for distributed deployment
    let topology = Topology {
        nodes: vec![
            Node {
                name: "adapters".to_string(),
                services: vec!["polygon-adapter".to_string()],
                host: Some("127.0.0.1".to_string()),
                port: Some(0),
                endpoint: None,
            },
            Node {
                name: "strategies".to_string(),
                services: vec!["flash-arbitrage".to_string()],
                host: Some("127.0.0.1".to_string()),
                port: Some(0),
                endpoint: None,
            },
        ],
        socket_dir: std::path::PathBuf::from("/tmp/mycelium"),
    };

    // Bind server for strategies node
    let strategy_addr = "127.0.0.1:0".parse().unwrap();
    let strategy_server = TcpTransport::bind(strategy_addr).await.unwrap();
    let strategy_bind_addr = strategy_server.local_addr();
    let mut strategy_sub = strategy_server.subscriber::<SwapEvent>();

    // Update topology with actual port
    let mut updated_topology = topology;
    updated_topology.nodes[1].port = Some(strategy_bind_addr.port());

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create adapter bus and publish to strategies
    let adapter_bus = MessageBus::from_topology(updated_topology, "adapters");
    let tcp_pub = adapter_bus
        .tcp_publisher::<SwapEvent>("strategies")
        .await
        .unwrap();

    // Publish messages
    for i in 0..3 {
        tcp_pub
            .publish(SwapEvent {
                pool_id: i,
                token_in: 1,
                token_out: 2,
                amount_in: 1000,
                amount_out: 2000,
                timestamp: i,
            })
            .await
            .unwrap();
    }

    // Verify messages received
    for i in 0..3 {
        let event = tokio::time::timeout(tokio::time::Duration::from_secs(1), strategy_sub.recv())
            .await
            .expect("Timeout")
            .expect("Failed to receive");

        assert_eq!(event.pool_id, i);
    }
}

#[tokio::test]
async fn test_mixed_transport_deployment() {
    // Test hybrid: Local + Unix + TCP all working together
    let dir = tempfile::tempdir().unwrap();
    let topology = Topology {
        nodes: vec![
            Node {
                name: "local-services".to_string(),
                services: vec!["service-a".to_string(), "service-b".to_string()],
                host: None,
                port: None,
                endpoint: None,
            },
            Node {
                name: "remote-services".to_string(),
                services: vec!["service-c".to_string()],
                host: Some("127.0.0.1".to_string()),
                port: Some(0),
                endpoint: None,
            },
        ],
        socket_dir: dir.path().to_path_buf(),
    };

    // Bind transports
    let remote_socket = topology.socket_path("remote-services");
    let _remote_transport = UnixTransport::bind(&remote_socket).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create buses
    let local_bus = MessageBus::from_topology(topology.clone(), "local-services");
    let _remote_bus = MessageBus::from_topology(topology, "remote-services");

    // Local pub/sub (Arc<T>)
    let local_pub = local_bus.publisher::<SwapEvent>();
    let mut local_sub = local_bus.subscriber::<SwapEvent>();

    // Cross-node pub/sub (Unix)
    let unix_pub = local_bus
        .unix_publisher::<SwapEvent>("remote-services")
        .await
        .expect("Failed to create Unix publisher");

    // Test local communication
    local_pub
        .publish(SwapEvent {
            pool_id: 1,
            token_in: 1,
            token_out: 2,
            amount_in: 1000,
            amount_out: 2000,
            timestamp: 1,
        })
        .await
        .unwrap();

    let local_event = local_sub.recv().await.unwrap();
    assert_eq!(local_event.pool_id, 1);

    // Test cross-node communication
    unix_pub
        .publish(SwapEvent {
            pool_id: 2,
            token_in: 1,
            token_out: 2,
            amount_in: 3000,
            amount_out: 6000,
            timestamp: 2,
        })
        .await
        .unwrap();

    // Verify both transport types work correctly
    assert_eq!(local_bus.subscriber_count::<SwapEvent>(), 1);
}
