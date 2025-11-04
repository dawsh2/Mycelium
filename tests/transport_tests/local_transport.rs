//! Local transport (Arc<T>) tests
//!
//! Tests for zero-copy in-process message passing.

use crate::common::*;
use mycelium_transport::{MessageBus, TransportConfig};
use std::sync::Arc;
use tokio::time::{sleep, Duration};

#[tokio::test]
async fn test_local_basic_pubsub() {
    let bus = MessageBus::new();

    let publisher = bus.publisher::<SwapEvent>();
    let mut subscriber = bus.subscriber::<SwapEvent>();

    let event = create_test_swap_event(123);
    publisher.publish(event).await.unwrap();

    let received = subscriber.recv().await.unwrap();
    assert_eq!(*received, event);
}

#[tokio::test]
async fn test_local_multiple_subscribers() {
    let bus = MessageBus::with_config(TransportConfig {
        channel_capacity: 1000,
    });

    let publisher = bus.publisher::<SwapEvent>();
    let mut subs: Vec<_> = (0..10).map(|_| bus.subscriber::<SwapEvent>()).collect();

    assert_eq!(bus.subscriber_count::<SwapEvent>(), 10);

    let event = create_test_swap_event(456);
    publisher.publish(event).await.unwrap();

    // All subscribers should receive the message
    for (i, mut sub) in subs.into_iter().enumerate() {
        let received = sub.recv().await.unwrap();
        assert_eq!(*received, create_test_swap_event(456));
        println!("Subscriber {} received message", i);
    }
}

#[tokio::test]
async fn test_local_mixed_message_types() {
    let bus = MessageBus::new();

    let swap_pub = bus.publisher::<SwapEvent>();
    let arb_pub = bus.publisher::<ArbitrageSignal>();
    let order_pub = bus.publisher::<OrderExecution>();

    let mut swap_sub = bus.subscriber::<SwapEvent>();
    let mut arb_sub = bus.subscriber::<ArbitrageSignal>();
    let mut order_sub = bus.subscriber::<OrderExecution>();

    // Publish different message types concurrently
    let swap_event = create_test_swap_event(100);
    let arb_signal = create_test_arbitrage_signal(200);
    let order_exec = create_test_order_execution(300, true);

    swap_pub.publish(swap_event).await.unwrap();
    arb_pub.publish(arb_signal).await.unwrap();
    order_pub.publish(order_exec).await.unwrap();

    // Each subscriber should only receive its message type
    let received_swap = swap_sub.recv().await.unwrap();
    let received_arb = arb_sub.recv().await.unwrap();
    let received_order = order_sub.recv().await.unwrap();

    assert_eq!(received_swap.pool_id, 100);
    assert_eq!(received_arb.opportunity_id, 200);
    assert_eq!(received_order.order_id, 300);
}

#[tokio::test]
async fn test_local_high_frequency_zero_copy() {
    let bus = MessageBus::with_config(TransportConfig {
        channel_capacity: 10000,
    });

    let publisher = bus.publisher::<SwapEvent>();
    let mut subscriber = bus.subscriber::<SwapEvent>();

    let message_count: u64 = 10000;
    let start_time = std::time::Instant::now();

    // Publish high frequency messages
    for i in 0..message_count {
        publisher.publish(create_test_swap_event(i)).await.unwrap();
    }

    // Receive all messages
    for i in 0..message_count {
        let received = subscriber.recv().await.unwrap();
        assert_eq!(*received, create_test_swap_event(i));
    }

    let metrics = PerformanceMetrics::new(message_count as usize, start_time.elapsed());

    // Local transport should be extremely fast (nanoseconds per message)
    metrics.assert_throughput_at_least(100000.0, "local transport");
    metrics.assert_latency_at_most(1, "local transport");

    println!(
        "Local transport: {} messages in {:?} ({:.2} msg/s, avg {}ns/msg)",
        metrics.message_count,
        metrics.duration,
        metrics.throughput,
        metrics.duration.as_nanos() as u64 / metrics.message_count as u64
    );
}

#[tokio::test]
async fn test_local_channel_backpressure() {
    let bus = MessageBus::with_config(TransportConfig {
        channel_capacity: 100, // Small capacity to test backpressure
    });

    let publisher = bus.publisher::<SwapEvent>();

    // Publish messages without subscribers to fill channel
    for i in 0..100 {
        publisher.publish(create_test_swap_event(i)).await.unwrap();
    }

    // Additional publishes should either work (if channel drops old messages)
    // or block (if channel waits for space). The important thing is no panic.
    for i in 100..110 {
        let result = publisher.publish(create_test_swap_event(i)).await;
        // Result depends on broadcast channel implementation
        match result {
            Ok(()) => println!("Message {} published successfully", i),
            Err(e) => println!("Message {} publish failed: {:?}", i, e),
        }
    }
}

#[tokio::test]
async fn test_local_subscriber_lifecycle() {
    let bus = MessageBus::new();
    let publisher = bus.publisher::<SwapEvent>();

    // Test subscriber creation and immediate drop
    {
        let _subscriber = bus.subscriber::<SwapEvent>();
        assert_eq!(bus.subscriber_count::<SwapEvent>(), 1);

        publisher.publish(create_test_swap_event(1)).await.unwrap();
    } // subscriber drops here

    // Allow cleanup time
    sleep(Duration::from_millis(10)).await;
    assert_eq!(bus.subscriber_count::<SwapEvent>(), 0);

    // Should still be able to publish
    publisher.publish(create_test_swap_event(2)).await.unwrap();

    // New subscriber should work
    let mut new_sub = bus.subscriber::<SwapEvent>();
    publisher.publish(create_test_swap_event(3)).await.unwrap();

    let received = new_sub.recv().await.unwrap();
    assert_eq!(*received, create_test_swap_event(3));
}

#[tokio::test]
async fn test_local_memory_efficiency() {
    use mycelium_protocol::impl_message;
    use zerocopy::{IntoBytes, FromBytes, FromZeros, Immutable};

    let bus = MessageBus::new();

    // Create large message to test memory sharing
    #[derive(Debug, Clone, Copy, PartialEq, IntoBytes, FromBytes, FromZeros, Immutable)]
    #[repr(C)]
    struct LargeMessage {
        data: [u64; 1000], // ~8KB per message
        id: u64,
    }

    impl_message!(LargeMessage, 99, "large-test");

    let publisher = bus.publisher::<LargeMessage>();
    let mut subscriber = bus.subscriber::<LargeMessage>();

    let mut large_msg = LargeMessage {
        data: [0; 1000],
        id: 42,
    };
    large_msg.data[0] = 123;

    publisher.publish(large_msg).await.unwrap();
    let received = subscriber.recv().await.unwrap();

    assert_eq!(received.id, 42);
    assert_eq!(received.data[0], 123);

    // Arc<T> should share the same memory, not copy
    println!("Large message test completed successfully");
}
