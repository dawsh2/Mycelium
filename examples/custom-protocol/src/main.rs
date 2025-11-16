// Include the generated protocol messages
include!(concat!(env!("OUT_DIR"), "/generated_messages.rs"));

use mycelium_transport::{codec, BufferPool, BufferPoolConfig};
use tokio::net::{UnixListener, UnixStream};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎮 Custom Protocol Example\n");
    println!("This demonstrates using mycelium-codegen to generate custom messages.\n");

    // Create buffer pool for zero-allocation message handling
    let buffer_pool_config = create_buffer_pool_config();
    let pool = BufferPool::new(BufferPoolConfig::from_map(buffer_pool_config));
    println!("✓ Buffer pool created with config for custom messages");

    // Create Unix socket for demo
    let socket_path = "/tmp/custom-protocol-demo.sock";
    let _ = std::fs::remove_file(socket_path); // Clean up any previous socket
    let listener = UnixListener::bind(socket_path)?;
    println!("✓ Listening on {}", socket_path);

    // Spawn a client to send messages
    let client_handle = tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        let mut client = UnixStream::connect(socket_path).await?;
        println!("\n📤 Client: Sending custom messages...\n");

        // Send PlayerLogin message
        let login = PlayerLogin {
            player_id: 12345,
            session_token: 0xDEADBEEF,
            timestamp: 1234567890,
        };
        codec::write_message(&mut client, &login).await?;
        println!(
            "  → Sent PlayerLogin (type_id={}, player_id={})",
            PlayerLogin::TYPE_ID,
            login.player_id
        );

        // Send PlayerPosition message
        let position = PlayerPosition {
            player_id: 12345,
            x: 100.5,
            y: 50.0,
            z: 200.75,
            rotation: 3.14159,
        };
        codec::write_message(&mut client, &position).await?;
        println!(
            "  → Sent PlayerPosition (type_id={}, pos=[{:.1}, {:.1}, {:.1}])",
            PlayerPosition::TYPE_ID,
            position.x,
            position.y,
            position.z
        );

        // Send GameState message
        let game_state = GameState {
            game_id: 999,
            tick: 54321,
            player_count: 42,
            elapsed_time: 3600,
        };
        codec::write_message(&mut client, &game_state).await?;
        println!(
            "  → Sent GameState (type_id={}, tick={}, players={})",
            GameState::TYPE_ID,
            game_state.tick,
            game_state.player_count
        );

        // Send ChatMessage
        let chat = ChatMessage {
            sender_id: 12345,
            channel_id: 1,
            message_hash: 0xCAFEBABE,
            timestamp: 1234567899,
        };
        codec::write_message(&mut client, &chat).await?;
        println!(
            "  → Sent ChatMessage (type_id={}, sender={})",
            ChatMessage::TYPE_ID,
            chat.sender_id
        );

        // Send ItemTransaction
        let transaction = ItemTransaction {
            transaction_id: 777,
            from_player: 12345,
            to_player: 67890,
            item_id: 42,
            quantity: 10,
        };
        codec::write_message(&mut client, &transaction).await?;
        println!(
            "  → Sent ItemTransaction (type_id={}, item={}, qty={})",
            ItemTransaction::TYPE_ID,
            transaction.item_id,
            transaction.quantity
        );

        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    });

    // Accept connection and receive messages
    let (mut stream, _) = listener.accept().await?;
    println!("\n📥 Server: Receiving messages...\n");

    // Read 5 messages
    for _ in 0..5 {
        let (type_id, buffer) = codec::read_frame_pooled(&mut stream, &pool).await?;

        // Dispatch based on type_id
        match type_id {
            PlayerLogin::TYPE_ID => {
                let payload = &buffer[mycelium_protocol::codec::HEADER_SIZE..];
                let msg: PlayerLogin = codec::deserialize_message(payload)?;
                println!(
                    "  ← Received PlayerLogin: player_id={}, token=0x{:X}, timestamp={}",
                    msg.player_id, msg.session_token, msg.timestamp
                );
            }
            PlayerPosition::TYPE_ID => {
                let payload = &buffer[mycelium_protocol::codec::HEADER_SIZE..];
                let msg: PlayerPosition = codec::deserialize_message(payload)?;
                println!("  ← Received PlayerPosition: player_id={}, pos=[{:.1}, {:.1}, {:.1}], rot={:.2}",
                    msg.player_id, msg.x, msg.y, msg.z, msg.rotation);
            }
            GameState::TYPE_ID => {
                let payload = &buffer[mycelium_protocol::codec::HEADER_SIZE..];
                let msg: GameState = codec::deserialize_message(payload)?;
                println!(
                    "  ← Received GameState: game_id={}, tick={}, players={}, elapsed={}s",
                    msg.game_id, msg.tick, msg.player_count, msg.elapsed_time
                );
            }
            ChatMessage::TYPE_ID => {
                let payload = &buffer[mycelium_protocol::codec::HEADER_SIZE..];
                let msg: ChatMessage = codec::deserialize_message(payload)?;
                println!(
                    "  ← Received ChatMessage: sender={}, channel={}, hash=0x{:X}",
                    msg.sender_id, msg.channel_id, msg.message_hash
                );
            }
            ItemTransaction::TYPE_ID => {
                let payload = &buffer[mycelium_protocol::codec::HEADER_SIZE..];
                let msg: ItemTransaction = codec::deserialize_message(payload)?;
                println!(
                    "  ← Received ItemTransaction: {} → {}, item={}, qty={}",
                    msg.from_player, msg.to_player, msg.item_id, msg.quantity
                );
            }
            _ => println!("  ← Unknown message type: {}", type_id),
        }
    }

    match client_handle.await {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => return Err(format!("Client error: {}", e).into()),
        Err(e) => return Err(format!("Join error: {}", e).into()),
    }

    // Show buffer pool stats
    let stats = pool.stats();
    println!("\n📊 Buffer Pool Stats:");
    println!("  Total acquires:    {}", stats.total_acquires);
    println!("  Total allocations: {}", stats.total_allocations);
    println!("  Hit rate:          {:.1}%", stats.hit_rate());
    println!("  Currently in use:  {}", stats.currently_in_use);

    println!("\n✅ Example completed successfully!");
    println!("\nThis example showed how to:");
    println!("  1. Define custom messages in contracts.yaml");
    println!("  2. Use mycelium-codegen in build.rs to generate Rust code");
    println!("  3. Send/receive custom messages with zero-copy serialization");
    println!("  4. Use buffer pools for zero-allocation message handling");

    // Clean up
    std::fs::remove_file(socket_path)?;

    Ok(())
}
