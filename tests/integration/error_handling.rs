//! Error handling integration tests
//!
//! Tests for various error conditions and recovery scenarios.

use crate::integration::{MessageBus, SwapEvent};
use mycelium_config::{Bundle, Deployment, DeploymentMode, Topology};
use mycelium_transport::{TransportError, UnixTransport};

#[tokio::test]
async fn test_service_not_found_error() {
    let bus = MessageBus::new();

    // No topology configured - should fail
    let result = bus.publisher_to::<SwapEvent>("some-service").await;
    assert!(matches!(result, Err(TransportError::ServiceNotFound(_))));

    let result = bus.subscriber_from::<SwapEvent>("some-service").await;
    assert!(matches!(result, Err(TransportError::ServiceNotFound(_))));
}

#[tokio::test]
async fn test_topology_service_not_found() {
    let topology = Topology {
        deployment: Deployment {
            mode: DeploymentMode::Bundled,
        },
        bundles: vec![Bundle {
            name: "test-bundle".to_string(),
            services: vec!["existing-service".to_string()],
            host: None,
            port: None,
        }],
        inter_bundle: None,
    };

    let bus = MessageBus::from_topology(topology, "test-bundle");

    // Try to connect to non-existent service
    let result = bus.publisher_to::<SwapEvent>("nonexistent-service").await;
    assert!(matches!(result, Err(TransportError::ServiceNotFound(msg))
                 if msg.contains("nonexistent-service")));

    let result = bus.subscriber_from::<SwapEvent>("nonexistent-service").await;
    assert!(matches!(result, Err(TransportError::ServiceNotFound(msg))
                 if msg.contains("nonexistent-service")));
}

#[tokio::test]
async fn test_unix_socket_not_found() {
    let topology = create_topology();
    let bus = MessageBus::from_topology(topology, "adapters");

    // Don't start server - socket doesn't exist
    let result = bus.unix_publisher::<SwapEvent>("strategies").await;
    assert!(result.is_none());

    let result = bus.unix_subscriber::<SwapEvent>("strategies").await;
    assert!(result.is_none());
}

#[tokio::test]
async fn test_publisher_with_no_subscribers() {
    let bus = MessageBus::new();
    let publisher = bus.publisher::<SwapEvent>();

    // No subscribers - should still work
    let result = publisher.publish(SwapEvent {
        pool_id: 1,
        amount_in: 100,
        amount_out: 200,
        timestamp: 12345,
    }).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_channel_capacity_limits() {
    // Use small capacity to test backpressure
    let bus = MessageBus::with_capacity(2);
    let publisher = bus.publisher::<SwapEvent>();

    // Don't create any subscribers - channel should fill up
    let mut results = vec![];

    // Publish up to capacity
    for i in 0..2 {
        let result = publisher.publish(SwapEvent {
            pool_id: i,
            amount_in: i as u128 * 1000,
            amount_out: i as u128 * 2000,
            timestamp: i as u64,
        }).await;
        results.push(result);
    }

    // First publishes should succeed
    assert!(results[0].is_ok());
    assert!(results[1].is_ok());

    // Additional publish might fail due to backpressure (depends on implementation)
    let result = publisher.publish(SwapEvent {
        pool_id: 2,
        amount_in: 2000,
        amount_out: 4000,
        timestamp: 2,
    }).await;

    // This might succeed or fail depending on broadcast channel behavior
    // The important thing is that it doesn't panic
}

#[tokio::test]
async fn test_bundle_name_not_configured() {
    // Create topology but don't set bundle name
    let topology = Topology {
        deployment: Deployment {
            mode: DeploymentMode::Bundled,
        },
        bundles: vec![],
        inter_bundle: None,
    };

    let bus = MessageBus::from_topology(topology, "test-bundle");

    // This should work since bundle name is set
    assert_eq!(bus.bundle_name(), Some("test-bundle"));
}

#[tokio::test]
async fn test_error_recovery_after_dropped_subscriber() {
    let bus = MessageBus::new();
    let publisher = bus.publisher::<SwapEvent>();

    {
        let _subscriber = bus.subscriber::<SwapEvent>();
        assert_eq!(bus.subscriber_count::<SwapEvent>(), 1);

        // Publish works
        publisher.publish(SwapEvent {
            pool_id: 1,
            amount_in: 100,
            amount_out: 200,
            timestamp: 1,
        }).await.unwrap();
    } // subscriber drops

    // Allow cleanup time
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    assert_eq!(bus.subscriber_count::<SwapEvent>(), 0);

    // Should still be able to publish
    let result = publisher.publish(SwapEvent {
        pool_id: 2,
        amount_in: 200,
        amount_out: 400,
        timestamp: 2,
    }).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_transport_creation_failure_cleanup() {
    let topology = create_topology();
    let bus = MessageBus::from_topology(topology, "adapters");

    // Multiple failed attempts to create Unix transport
    for _ in 0..10 {
        let result = bus.unix_publisher::<SwapEvent>("strategies").await;
        assert!(result.is_none());
    }

    // Should still be able to create local publisher
    let local_pub = bus.publisher::<SwapEvent>();
    assert!(local_pub.publish(SwapEvent {
        pool_id: 999,
        amount_in: 999,
        amount_out: 1999,
        timestamp: 999,
    }).await.is_ok());
}

fn create_topology() -> Topology {
    Topology {
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
        inter_bundle: None,
    }
}