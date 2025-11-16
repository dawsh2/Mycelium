use mycelium_protocol::{CounterUpdate, Message, TextMessage};
use mycelium_transport::{BufferPool, BufferPoolConfig, TcpTransport, UnixTransport};
use std::collections::HashMap;
use tempfile::tempdir;

/// Test TCP transport with buffer pool using generic message types
#[tokio::test]
async fn test_tcp_with_buffer_pool_high_throughput() {
    // Create buffer pool config
    let mut pool_config_map = HashMap::new();
    pool_config_map.insert(TextMessage::TYPE_ID as usize, 10);
    let pool_config = BufferPoolConfig::from_map(pool_config_map);
    let pool = BufferPool::new(pool_config.clone());

    // Create TCP transport with buffer pool
    let addr = "127.0.0.1:0".parse().unwrap();
    let server = TcpTransport::bind_with_buffer_pool(addr, Some(pool.clone()))
        .await
        .unwrap();
    let server_addr = server.local_addr();

    // Create subscriber
    let mut sub = server.subscriber::<TextMessage>();

    // Give server time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Connect client
    let client = TcpTransport::connect(server_addr).await.unwrap();
    let publisher = client.publisher::<TextMessage>().unwrap();

    // Send 100 messages (should result in high hit rate)
    for i in 0..100 {
        let msg = TextMessage {
            sender: format!("Sender{}", i).try_into().unwrap(),
            content: format!("Message content #{}", i).try_into().unwrap(),
            timestamp: 1000 + i,
        };

        publisher.publish(msg).await.unwrap();
    }

    // Receive all messages
    for i in 0..100 {
        let msg = tokio::time::timeout(tokio::time::Duration::from_secs(2), sub.recv())
            .await
            .expect("Timeout")
            .expect("Message");

        assert_eq!(msg.timestamp, 1000 + i);
        assert_eq!(msg.sender.as_str().unwrap(), &format!("Sender{}", i));
    }

    // Verify buffer pool statistics
    let stats = pool.stats();
    println!("TCP Buffer Pool Stats after 100 messages:");
    println!("  Total allocations: {}", stats.total_allocations);
    println!("  Total acquires: {}", stats.total_acquires);
    println!("  Total returns: {}", stats.total_returns);
    println!("  Hit rate: {:.2}%", stats.hit_rate());
    println!("  Currently in use: {}", stats.currently_in_use);

    // Verify high hit rate (>80%)
    assert!(
        stats.hit_rate() > 80.0,
        "Hit rate should be >80%, got {:.2}%",
        stats.hit_rate()
    );

    // Verify no memory leaks (all buffers returned)
    assert_eq!(
        stats.currently_in_use, 0,
        "All buffers should be returned to pool"
    );

    // Verify only a few allocations needed
    assert!(
        stats.total_allocations < 10,
        "Should only allocate a few buffers, got {}",
        stats.total_allocations
    );
}

/// Test Unix transport with buffer pool using CounterUpdate messages
#[tokio::test]
async fn test_unix_with_buffer_pool_mixed_messages() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("test_buffer_pool.sock");

    // Create buffer pool config
    let mut pool_config_map = HashMap::new();
    pool_config_map.insert(CounterUpdate::TYPE_ID as usize, 10);
    let pool_config = BufferPoolConfig::from_map(pool_config_map);
    let pool = BufferPool::new(pool_config.clone());

    // Create Unix transport with buffer pool
    let server = UnixTransport::bind_with_buffer_pool(&socket_path, Some(pool.clone()))
        .await
        .unwrap();

    let mut sub = server.subscriber::<CounterUpdate>();

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Connect client
    let client = UnixTransport::connect(&socket_path).await.unwrap();
    let publisher = client.publisher::<CounterUpdate>().unwrap();

    // Send 100 counter updates
    for i in 0..100 {
        let msg = CounterUpdate {
            counter_id: (i % 10) as u64,
            value: i as i32,
            delta: 1,
        };
        publisher.publish(msg).await.unwrap();
    }

    // Receive all 100 messages
    let mut total_value = 0i32;

    for _ in 0..100 {
        let msg = tokio::time::timeout(tokio::time::Duration::from_secs(2), sub.recv())
            .await
            .expect("Timeout")
            .expect("Message");

        total_value += msg.value;
    }

    // Sum of 0..100 = 4950
    assert_eq!(total_value, 4950);

    // Verify buffer pool statistics
    let stats = pool.stats();
    println!("Unix Buffer Pool Stats after 100 CounterUpdate messages:");
    println!("  Total allocations: {}", stats.total_allocations);
    println!("  Total acquires: {}", stats.total_acquires);
    println!("  Hit rate: {:.2}%", stats.hit_rate());

    assert!(stats.hit_rate() > 80.0);
    assert_eq!(stats.currently_in_use, 0);
}

/// Test buffer pool with TextMessage messages (variable-length content)
#[tokio::test]
async fn test_buffer_pool_with_text_messages() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("test_text.sock");

    // Create buffer pool
    let mut pool_config_map = HashMap::new();
    pool_config_map.insert(TextMessage::TYPE_ID as usize, 10);
    let pool_config = BufferPoolConfig::from_map(pool_config_map);
    let pool = BufferPool::new(pool_config);

    let server = UnixTransport::bind_with_buffer_pool(&socket_path, Some(pool.clone()))
        .await
        .unwrap();

    let mut sub = server.subscriber::<TextMessage>();

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let client = UnixTransport::connect(&socket_path).await.unwrap();
    let publisher = client.publisher::<TextMessage>().unwrap();

    // Send messages with different content lengths
    for i in 0..30 {
        let content = match i % 3 {
            0 => "Short".to_string(),
            1 => "Medium length message".to_string(),
            _ => "This is a longer message with more content to test buffer pool efficiency"
                .to_string(),
        };

        let msg = TextMessage {
            sender: format!("User{}", i).try_into().unwrap(),
            content: content.try_into().unwrap(),
            timestamp: 1000 + i,
        };

        publisher.publish(msg).await.unwrap();
    }

    // Receive all messages
    for i in 0..30 {
        let msg = tokio::time::timeout(tokio::time::Duration::from_secs(2), sub.recv())
            .await
            .expect("Timeout")
            .expect("Message");

        assert_eq!(msg.timestamp, 1000 + i);
        assert_eq!(msg.sender.as_str().unwrap(), &format!("User{}", i));
    }

    let stats = pool.stats();
    println!("TextMessage Buffer Pool Stats after 30 messages:");
    println!("  Total allocations: {}", stats.total_allocations);
    println!("  Hit rate: {:.2}%", stats.hit_rate());

    // All TextMessage messages use the same size class
    // so we should have very high hit rate
    assert!(stats.hit_rate() > 90.0);
    assert_eq!(stats.currently_in_use, 0);
}

/// Test that buffer pool doesn't leak memory under sustained load
#[tokio::test]
async fn test_buffer_pool_no_memory_leak() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("test_leak.sock");

    let mut pool_config_map = HashMap::new();
    pool_config_map.insert(TextMessage::TYPE_ID as usize, 10);
    let pool_config = BufferPoolConfig::from_map(pool_config_map);
    let pool = BufferPool::new(pool_config);

    let server = UnixTransport::bind_with_buffer_pool(&socket_path, Some(pool.clone()))
        .await
        .unwrap();

    let mut sub = server.subscriber::<TextMessage>();

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let client = UnixTransport::connect(&socket_path).await.unwrap();
    let publisher = client.publisher::<TextMessage>().unwrap();

    // Send 1000 messages in bursts
    for batch in 0..10 {
        for i in 0..100 {
            let msg = TextMessage {
                sender: format!("Batch{}User{}", batch, i).try_into().unwrap(),
                content: "Test message for leak detection".try_into().unwrap(),
                timestamp: 3000 + (batch * 100 + i),
            };

            publisher.publish(msg).await.unwrap();
        }

        // Receive batch
        for _ in 0..100 {
            let _ = tokio::time::timeout(tokio::time::Duration::from_secs(2), sub.recv())
                .await
                .expect("Timeout")
                .expect("Message");
        }

        // Check stats after each batch
        let stats = pool.stats();
        assert_eq!(
            stats.currently_in_use, 0,
            "Batch {}: All buffers should be returned",
            batch
        );
    }

    let final_stats = pool.stats();
    println!("Final stats after 1000 messages:");
    println!("  Total allocations: {}", final_stats.total_allocations);
    println!("  Total acquires: {}", final_stats.total_acquires);
    println!("  Total returns: {}", final_stats.total_returns);
    println!("  Hit rate: {:.2}%", final_stats.hit_rate());
    println!("  Currently in use: {}", final_stats.currently_in_use);

    // Verify high efficiency
    assert_eq!(final_stats.total_acquires, 1000);
    assert!(final_stats.hit_rate() > 95.0);
    assert_eq!(final_stats.currently_in_use, 0);

    // Should only allocate a handful of buffers
    assert!(
        final_stats.total_allocations < 10,
        "Only need a few buffers, got {}",
        final_stats.total_allocations
    );
}

/// Benchmark: Compare performance with and without buffer pool
#[tokio::test]
#[ignore] // Run with --ignored for benchmarks
async fn bench_buffer_pool_performance() {
    use std::time::Instant;

    let dir = tempdir().unwrap();

    // Test WITHOUT buffer pool
    let socket_path1 = dir.path().join("bench_no_pool.sock");
    let server1 = UnixTransport::bind(&socket_path1).await.unwrap();
    let mut sub1 = server1.subscriber::<TextMessage>();
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let client1 = UnixTransport::connect(&socket_path1).await.unwrap();
    let pub1 = client1.publisher::<TextMessage>().unwrap();

    let start = Instant::now();
    for i in 0..1000 {
        let msg = TextMessage {
            sender: format!("Bench{}", i).try_into().unwrap(),
            content: "Benchmark message".try_into().unwrap(),
            timestamp: 4000 + i,
        };
        pub1.publish(msg).await.unwrap();
    }
    for _ in 0..1000 {
        let _ = sub1.recv().await;
    }
    let duration_no_pool = start.elapsed();

    // Test WITH buffer pool
    let socket_path2 = dir.path().join("bench_with_pool.sock");
    let mut pool_config_map = HashMap::new();
    pool_config_map.insert(TextMessage::TYPE_ID as usize, 10);
    let pool_config = BufferPoolConfig::from_map(pool_config_map);
    let pool = BufferPool::new(pool_config);
    let server2 = UnixTransport::bind_with_buffer_pool(&socket_path2, Some(pool.clone()))
        .await
        .unwrap();
    let mut sub2 = server2.subscriber::<TextMessage>();
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let client2 = UnixTransport::connect(&socket_path2).await.unwrap();
    let pub2 = client2.publisher::<TextMessage>().unwrap();

    let start = Instant::now();
    for i in 0..1000 {
        let msg = TextMessage {
            sender: format!("Bench{}", i).try_into().unwrap(),
            content: "Benchmark message".try_into().unwrap(),
            timestamp: 4000 + i,
        };
        pub2.publish(msg).await.unwrap();
    }
    for _ in 0..1000 {
        let _ = sub2.recv().await;
    }
    let duration_with_pool = start.elapsed();

    println!("Performance comparison (1000 messages):");
    println!("  Without pool: {:?}", duration_no_pool);
    println!("  With pool:    {:?}", duration_with_pool);
    println!(
        "  Speedup:      {:.2}x",
        duration_no_pool.as_secs_f64() / duration_with_pool.as_secs_f64()
    );

    let stats = pool.stats();
    println!("  Hit rate:     {:.2}%", stats.hit_rate());
}
