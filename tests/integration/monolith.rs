//! Monolith deployment integration tests
//!
//! All services run in the same process with Arc<T> transport only.

use crate::integration::{ArbitrageSignal, MessageBus, OrderExecution, SwapEvent};
use mycelium_transport::Publisher;
use tokio::time::{sleep, Duration};

#[tokio::test]
async fn test_basic_pubsub() {
    let bus = MessageBus::new();

    let publisher = bus.publisher::<SwapEvent>();
    let mut subscriber = bus.subscriber::<SwapEvent>();

    // Publish and receive
    let event = SwapEvent {
        pool_id: 123,
        amount_in: 1000,
        amount_out: 2000,
        timestamp: 123456789,
    };

    publisher.publish(event).await.unwrap();
    let received = subscriber.recv().await.unwrap();

    assert_eq!(*received, event);
}

#[tokio::test]
async fn test_multiple_subscribers() {
    let bus = MessageBus::new();

    let publisher = bus.publisher::<SwapEvent>();
    let mut sub1 = bus.subscriber::<SwapEvent>();
    let mut sub2 = bus.subscriber::<SwapEvent>();
    let mut sub3 = bus.subscriber::<SwapEvent>();

    assert_eq!(bus.subscriber_count::<SwapEvent>(), 3);

    let event = SwapEvent {
        pool_id: 456,
        amount_in: 500,
        amount_out: 1000,
        timestamp: 987654321,
    };

    publisher.publish(event).await.unwrap();

    // All subscribers should receive the message
    let received1 = sub1.recv().await.unwrap();
    let received2 = sub2.recv().await.unwrap();
    let received3 = sub3.recv().await.unwrap();

    assert_eq!(*received1, event);
    assert_eq!(*received2, event);
    assert_eq!(*received3, event);
}

#[tokio::test]
async fn test_multiple_message_types() {
    let bus = MessageBus::new();

    let swap_pub = bus.publisher::<SwapEvent>();
    let arb_pub = bus.publisher::<ArbitrageSignal>();
    let order_pub = bus.publisher::<OrderExecution>();

    let mut swap_sub = bus.subscriber::<SwapEvent>();
    let mut arb_sub = bus.subscriber::<ArbitrageSignal>();
    let mut order_sub = bus.subscriber::<OrderExecution>();

    // Publish different message types
    swap_pub.publish(SwapEvent {
        pool_id: 1,
        amount_in: 100,
        amount_out: 200,
        timestamp: 1000,
    }).await.unwrap();

    arb_pub.publish(ArbitrageSignal {
        opportunity_id: 42,
        profit: 5000,
        timestamp: 2000,
    }).await.unwrap();

    order_pub.publish(OrderExecution {
        order_id: 999,
        success: 1,  // 1 = true
        timestamp: 3000,
    }).await.unwrap();

    // Each subscriber should only receive its message type
    let swap_received = swap_sub.recv().await.unwrap();
    let arb_received = arb_sub.recv().await.unwrap();
    let order_received = order_sub.recv().await.unwrap();

    assert_eq!(swap_received.pool_id, 1);
    assert_eq!(arb_received.opportunity_id, 42);
    assert_eq!(order_received.order_id, 999);
}

#[tokio::test]
async fn test_high_frequency_messages() {
    let bus = MessageBus::with_capacity(10000); // Larger capacity for high frequency

    let publisher = bus.publisher::<SwapEvent>();
    let mut subscriber = bus.subscriber::<SwapEvent>();

    let message_count = 1000;
    let start_time = std::time::Instant::now();

    // Publish high frequency messages
    for i in 0..message_count {
        publisher.publish(SwapEvent {
            pool_id: i,
            amount_in: i as u128 * 1000,
            amount_out: i as u128 * 2000,
            timestamp: i as u64,
        }).await.unwrap();
    }

    // Receive all messages
    for i in 0..message_count {
        let received = subscriber.recv().await.unwrap();
        assert_eq!(received.pool_id, i);
    }

    let elapsed = start_time.elapsed();
    println!("Published and received {} messages in {:?}", message_count, elapsed);

    // Should be very fast (Arc<T> + no serialization)
    assert!(elapsed.as_millis() < 100); // Less than 100ms for 1000 messages
}

#[tokio::test]
async fn test_subscriber_drop_handling() {
    let bus = MessageBus::new();

    let publisher = bus.publisher::<SwapEvent>();

    {
        let _subscriber = bus.subscriber::<SwapEvent>();
        assert_eq!(bus.subscriber_count::<SwapEvent>(), 1);

        // Publish should work
        publisher.publish(SwapEvent {
            pool_id: 1,
            amount_in: 100,
            amount_out: 200,
            timestamp: 1,
        }).await.unwrap();
    } // subscriber drops here

    // Give some time for cleanup
    sleep(Duration::from_millis(10)).await;

    assert_eq!(bus.subscriber_count::<SwapEvent>(), 0);

    // Should still be able to publish (just no one receives)
    publisher.publish(SwapEvent {
        pool_id: 2,
        amount_in: 200,
        amount_out: 400,
        timestamp: 2,
    }).await.unwrap();
}