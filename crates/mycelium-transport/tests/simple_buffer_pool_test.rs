use mycelium_protocol::{create_buffer_pool_config, InstrumentMeta};
use mycelium_transport::{BufferPool, BufferPoolConfig, UnixTransport};
use tempfile::tempdir;

#[tokio::test]
async fn test_simple_buffer_pool() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("simple_test.sock");

    // Create buffer pool
    let pool_config = BufferPoolConfig::from_map(create_buffer_pool_config());
    let pool = BufferPool::new(pool_config);

    // Create server with buffer pool
    let server = UnixTransport::bind_with_buffer_pool(&socket_path, Some(pool.clone()))
        .await
        .unwrap();

    let mut sub = server.subscriber::<InstrumentMeta>();

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Connect client
    let client = UnixTransport::connect(&socket_path).await.unwrap();
    let publisher = client.publisher::<InstrumentMeta>().unwrap();

    // Send ONE message
    let addr = [1u8; 20];
    let msg = InstrumentMeta::new(addr, "WETH", 18, 137).unwrap();
    println!("Publishing message...");
    publisher.publish(msg).await.unwrap();
    println!("Message published");

    // Try to receive with extended timeout
    println!("Waiting for message...");
    match tokio::time::timeout(tokio::time::Duration::from_secs(5), sub.recv()).await {
        Ok(Some(received)) => {
            println!("Message received!");
            println!("  Symbol: {}", received.symbol_str());
            println!("  Decimals: {}", received.decimals);

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
