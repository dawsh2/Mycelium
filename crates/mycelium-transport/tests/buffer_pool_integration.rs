use mycelium_protocol::{
    create_buffer_pool_config, ArbitrageSignal, InstrumentMeta, PoolStateUpdate, U256,
};
use mycelium_transport::{BufferPool, BufferPoolConfig, TcpTransport, UnixTransport};
use tempfile::tempdir;

/// Test TCP transport with buffer pool using real generated message types
#[tokio::test]
async fn test_tcp_with_buffer_pool_high_throughput() {
    // Create buffer pool from generated config
    let pool_config = BufferPoolConfig::from_map(create_buffer_pool_config());
    let pool = BufferPool::new(pool_config.clone());

    // Create TCP transport with buffer pool
    let addr = "127.0.0.1:0".parse().unwrap();
    let server = TcpTransport::bind_with_buffer_pool(addr, Some(pool.clone()))
        .await
        .unwrap();
    let server_addr = server.local_addr();

    // Create subscriber
    let mut sub = server.subscriber::<InstrumentMeta>();

    // Give server time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Connect client
    let client = TcpTransport::connect(server_addr).await.unwrap();
    let publisher = client.publisher::<InstrumentMeta>().unwrap();

    // Send 100 messages (should result in high hit rate)
    for i in 0..100 {
        let mut addr = [1u8; 20]; // Non-zero address
        addr[0] = (i + 1) as u8; // Make unique
        let msg = InstrumentMeta::new(addr, &format!("TOKEN{}", i), 18, 137).unwrap();

        publisher.publish(msg).await.unwrap();
    }

    // Receive all messages
    for i in 0..100 {
        let msg = tokio::time::timeout(tokio::time::Duration::from_secs(2), sub.recv())
            .await
            .expect("Timeout")
            .expect("Message");

        assert_eq!(msg.decimals, 18);
        assert_eq!(msg.chain_id, 137);
        assert_eq!(msg.symbol_str(), &format!("TOKEN{}", i));
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

/// Test Unix transport with buffer pool using PoolStateUpdate messages
#[tokio::test]
async fn test_unix_with_buffer_pool_mixed_messages() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("test_buffer_pool.sock");

    // Create buffer pool from generated config
    let pool_config = BufferPoolConfig::from_map(create_buffer_pool_config());
    let pool = BufferPool::new(pool_config.clone());

    // Create Unix transport with buffer pool
    let server = UnixTransport::bind_with_buffer_pool(&socket_path, Some(pool.clone()))
        .await
        .unwrap();

    let mut sub = server.subscriber::<PoolStateUpdate>();

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Connect client
    let client = UnixTransport::connect(&socket_path).await.unwrap();
    let publisher = client.publisher::<PoolStateUpdate>().unwrap();

    // Send mix of V2 and V3 pool state updates (50 each)
    for i in 0..50 {
        // V2 pool
        let mut addr1 = [1u8; 20];
        addr1[0] = (i + 1) as u8;
        let msg = PoolStateUpdate::new_v2(
            addr1,
            1,
            U256::from(1000 * (i + 1)),
            U256::from(2000 * (i + 1)),
            1000 + i,
        )
        .unwrap();
        publisher.publish(msg).await.unwrap();

        // V3 pool
        let mut addr2 = [2u8; 20];
        addr2[0] = (i + 51) as u8;
        let msg = PoolStateUpdate::new_v3(
            addr2,
            2,
            U256::from(50000 * (i + 1)),
            U256::from(123456789),
            100,
            2000 + i,
        )
        .unwrap();
        publisher.publish(msg).await.unwrap();
    }

    // Receive all 100 messages
    let mut v2_count = 0;
    let mut v3_count = 0;

    for _ in 0..100 {
        let msg = tokio::time::timeout(tokio::time::Duration::from_secs(2), sub.recv())
            .await
            .expect("Timeout")
            .expect("Message");

        if msg.is_v2() {
            v2_count += 1;
        }
        if msg.is_v3() {
            v3_count += 1;
        }
    }

    assert_eq!(v2_count, 50);
    assert_eq!(v3_count, 50);

    // Verify buffer pool statistics
    let stats = pool.stats();
    println!("Unix Buffer Pool Stats after 100 PoolStateUpdate messages:");
    println!("  Total allocations: {}", stats.total_allocations);
    println!("  Total acquires: {}", stats.total_acquires);
    println!("  Hit rate: {:.2}%", stats.hit_rate());

    assert!(stats.hit_rate() > 80.0);
    assert_eq!(stats.currently_in_use, 0);
}

/// Test buffer pool with ArbitrageSignal messages (variable-length paths)
#[tokio::test]
async fn test_buffer_pool_with_arbitrage_signals() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("test_arbitrage.sock");

    // Create buffer pool
    let pool_config = BufferPoolConfig::from_map(create_buffer_pool_config());
    let pool = BufferPool::new(pool_config);

    let server = UnixTransport::bind_with_buffer_pool(&socket_path, Some(pool.clone()))
        .await
        .unwrap();

    let mut sub = server.subscriber::<ArbitrageSignal>();

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let client = UnixTransport::connect(&socket_path).await.unwrap();
    let publisher = client.publisher::<ArbitrageSignal>().unwrap();

    // Send signals with different path lengths (2, 3, 4 hops)
    for i in 0..30 {
        let addr1 = [1u8; 20];
        let addr2 = [2u8; 20];
        let addr3 = [3u8; 20];
        let addr4 = [4u8; 20];

        let path = match i % 3 {
            0 => vec![addr1, addr2],               // 2 hops
            1 => vec![addr1, addr2, addr3],        // 3 hops
            _ => vec![addr1, addr2, addr3, addr4], // 4 hops
        };

        let msg =
            ArbitrageSignal::new(i, &path, 100.5 + i as f64, U256::from(21000), 1000 + i).unwrap();

        publisher.publish(msg).await.unwrap();
    }

    // Receive all messages
    for i in 0..30 {
        let msg = tokio::time::timeout(tokio::time::Duration::from_secs(2), sub.recv())
            .await
            .expect("Timeout")
            .expect("Message");

        assert_eq!(msg.opportunity_id, i);
        assert_eq!(msg.estimated_profit_usd, 100.5 + i as f64);
    }

    let stats = pool.stats();
    println!("ArbitrageSignal Buffer Pool Stats after 30 messages:");
    println!("  Total allocations: {}", stats.total_allocations);
    println!("  Hit rate: {:.2}%", stats.hit_rate());

    // All ArbitrageSignal messages use the same size class (256 bytes)
    // so we should have very high hit rate
    assert!(stats.hit_rate() > 90.0);
    assert_eq!(stats.currently_in_use, 0);
}

/// Test that buffer pool doesn't leak memory under sustained load
#[tokio::test]
async fn test_buffer_pool_no_memory_leak() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("test_leak.sock");

    let pool_config = BufferPoolConfig::from_map(create_buffer_pool_config());
    let pool = BufferPool::new(pool_config);

    let server = UnixTransport::bind_with_buffer_pool(&socket_path, Some(pool.clone()))
        .await
        .unwrap();

    let mut sub = server.subscriber::<InstrumentMeta>();

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let client = UnixTransport::connect(&socket_path).await.unwrap();
    let publisher = client.publisher::<InstrumentMeta>().unwrap();

    // Send 1000 messages in bursts
    for batch in 0..10 {
        for i in 0..100 {
            let mut addr = [1u8; 20];
            addr[0] = ((batch * 100 + i) % 255 + 1) as u8; // Ensure non-zero
            let msg = InstrumentMeta::new(addr, "WETH", 18, 137).unwrap();

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
    let mut sub1 = server1.subscriber::<InstrumentMeta>();
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let client1 = UnixTransport::connect(&socket_path1).await.unwrap();
    let pub1 = client1.publisher::<InstrumentMeta>().unwrap();

    let start = Instant::now();
    for i in 0..1000 {
        let mut addr = [1u8; 20];
        addr[0] = (i % 255 + 1) as u8;
        let msg = InstrumentMeta::new(addr, "WETH", 18, 137).unwrap();
        pub1.publish(msg).await.unwrap();
    }
    for _ in 0..1000 {
        let _ = sub1.recv().await;
    }
    let duration_no_pool = start.elapsed();

    // Test WITH buffer pool
    let socket_path2 = dir.path().join("bench_with_pool.sock");
    let pool_config = BufferPoolConfig::from_map(create_buffer_pool_config());
    let pool = BufferPool::new(pool_config);
    let server2 = UnixTransport::bind_with_buffer_pool(&socket_path2, Some(pool.clone()))
        .await
        .unwrap();
    let mut sub2 = server2.subscriber::<InstrumentMeta>();
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let client2 = UnixTransport::connect(&socket_path2).await.unwrap();
    let pub2 = client2.publisher::<InstrumentMeta>().unwrap();

    let start = Instant::now();
    for i in 0..1000 {
        let mut addr = [1u8; 20];
        addr[0] = (i % 255 + 1) as u8;
        let msg = InstrumentMeta::new(addr, "WETH", 18, 137).unwrap();
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
