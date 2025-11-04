/// Simple pub/sub example demonstrating zero-copy message passing
///
/// This is a generic example that works for any domain.
/// For domain-specific examples, see:
/// - examples/generic/ (IoT, gaming, etc.)
/// - examples/domain_specific/ (DeFi, crypto trading)
///
/// Run with: cargo run --example simple_pubsub
use mycelium_protocol::{impl_message, Message};
use mycelium_transport::MessageBus;
use zerocopy::{IntoBytes, FromBytes, FromZeros, Immutable};

// Define a generic event message (C layout for zerocopy)
#[derive(Debug, Clone, Copy, PartialEq, IntoBytes, FromBytes, FromZeros, Immutable)]
#[repr(C)]
struct DataEvent {
    source_id: u64,
    value_a: u64,
    value_b: u64,
    timestamp: u64,
}

// Implement Message trait
impl_message!(DataEvent, 1, "events");

#[tokio::main]
async fn main() {
    println!("=== Mycelium: Simple Pub/Sub Example ===\n");

    // Create message bus (local transport with Arc<T>)
    let bus = MessageBus::new();

    // Create publisher
    let publisher = bus.publisher::<DataEvent>();

    // Create multiple subscribers
    let mut subscriber1 = bus.subscriber::<DataEvent>();
    let mut subscriber2 = bus.subscriber::<DataEvent>();

    println!("Created 1 publisher and 2 subscribers");
    println!("Subscriber count: {}\n", bus.subscriber_count::<DataEvent>());

    // Spawn subscriber tasks
    let sub1_handle = tokio::spawn(async move {
        println!("[Subscriber 1] Waiting for events...");
        while let Some(event) = subscriber1.recv().await {
            println!(
                "[Subscriber 1] Received: source={}, value_a={}, timestamp={}",
                event.source_id, event.value_a, event.timestamp
            );
        }
    });

    let sub2_handle = tokio::spawn(async move {
        println!("[Subscriber 2] Waiting for events...");
        while let Some(event) = subscriber2.recv().await {
            println!(
                "[Subscriber 2] Received: source={}, value_b={}, timestamp={}",
                event.source_id, event.value_b, event.timestamp
            );
        }
    });

    // Give subscribers time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Publish some events
    println!("\n[Publisher] Publishing events...\n");

    for i in 1..=3 {
        let event = DataEvent {
            source_id: i,
            value_a: 1000 * i as u64,
            value_b: 900 * i as u64,
            timestamp: 10000 + i,
        };

        publisher.publish(event).await.unwrap();
        println!("[Publisher] Published event {}", i);

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Close publisher (drops it)
    drop(publisher);

    // Wait for subscribers to finish
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    println!("\n=== Example Complete ===");
    println!("All messages delivered via Arc<T> (zero-copy)");
}
