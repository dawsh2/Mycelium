use mycelium_protocol::{Message, TextMessage};
use mycelium_transport::{BufferPool, BufferPoolConfig, UnixTransport};
use std::collections::HashMap;
use tempfile::tempdir;

#[tokio::test]
async fn test_simple_buffer_pool() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("simple_test.sock");

    // Create buffer pool with generic message types
    let mut pool_config_map = HashMap::new();
    pool_config_map.insert(TextMessage::TYPE_ID as usize, 10); // 10 buffers for TextMessage
    let pool_config = BufferPoolConfig::from_map(pool_config_map);
    let pool = BufferPool::new(pool_config);

    // Create server with buffer pool
    let server = UnixTransport::bind_with_buffer_pool(&socket_path, Some(pool.clone()))
        .await
        .unwrap();

    let mut sub = server.subscriber::<TextMessage>();

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Connect client
    let client = UnixTransport::connect(&socket_path).await.unwrap();
    let publisher = client.publisher::<TextMessage>().unwrap();

    // Send ONE message
    let msg = TextMessage {
        sender: "Alice".try_into().unwrap(),
        content: "Hello from buffer pool test!".try_into().unwrap(),
        timestamp: 1000,
    };
    println!("Publishing message...");
    publisher.publish(msg).await.unwrap();
    println!("Message published");

    // Try to receive with extended timeout
    println!("Waiting for message...");
    match tokio::time::timeout(tokio::time::Duration::from_secs(5), sub.recv()).await {
        Ok(Some(received)) => {
            println!("Message received!");
            println!("  Sender: {}", received.sender.as_str().unwrap());
            println!("  Content: {}", received.content.as_str().unwrap());

            let stats = pool.stats();
            println!("Buffer Pool Stats:");
            println!("  Allocations: {}", stats.total_allocations);
            println!("  Acquires: {}", stats.total_acquires);
            println!("  Returns: {}", stats.total_returns);
            println!("  In use: {}", stats.currently_in_use);
        }
        Ok(None) => {
            panic!("Channel closed");
        }
        Err(_) => {
            panic!("Timeout waiting for message");
        }
    }
}
