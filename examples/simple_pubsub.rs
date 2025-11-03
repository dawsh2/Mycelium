/// Simple pub/sub example demonstrating zero-copy message passing
///
/// Run with: cargo run --example simple_pubsub
use mycelium_protocol::{impl_message, Message};
use mycelium_transport::MessageBus;
use zerocopy::{AsBytes, FromBytes, FromZeroes};

// Define a message type (C layout for zerocopy)
#[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
struct SwapEvent {
    pool_address: u64,
    amount_in: u64,
    amount_out: u64,
    block_number: u64,
}

// Implement Message trait
impl_message!(SwapEvent, 11, "market-data");

#[tokio::main]
async fn main() {
    println!("=== Mycelium Message Bus Example ===\n");

    // Create message bus (local transport with Arc<T>)
    let bus = MessageBus::new();

    // Create publisher
    let publisher = bus.publisher::<SwapEvent>();

    // Create multiple subscribers
    let mut subscriber1 = bus.subscriber::<SwapEvent>();
    let mut subscriber2 = bus.subscriber::<SwapEvent>();

    println!("Created 1 publisher and 2 subscribers");
    println!("Subscriber count: {}\n", bus.subscriber_count::<SwapEvent>());

    // Spawn subscriber tasks
    let sub1_handle = tokio::spawn(async move {
        println!("[Subscriber 1] Waiting for events...");
        while let Some(event) = subscriber1.recv().await {
            println!(
                "[Subscriber 1] Received: pool={}, amount_in={}, block={}",
                event.pool_address, event.amount_in, event.block_number
            );
        }
    });

    let sub2_handle = tokio::spawn(async move {
        println!("[Subscriber 2] Waiting for events...");
        while let Some(event) = subscriber2.recv().await {
            println!(
                "[Subscriber 2] Received: pool={}, amount_out={}, block={}",
                event.pool_address, event.amount_out, event.block_number
            );
        }
    });

    // Give subscribers time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Publish some events
    println!("\n[Publisher] Publishing events...\n");

    for i in 1..=3 {
        let event = SwapEvent {
            pool_address: i,
            amount_in: 1000 * i as u64,
            amount_out: 900 * i as u64,
            block_number: 10000 + i,
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
