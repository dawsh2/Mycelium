/// Generic game server example - real-time multiplayer messaging
///
/// Demonstrates Mycelium for game state synchronization without DeFi terminology.
/// Works for any multiplayer game: FPS, MMO, real-time strategy, etc.
///
/// Run with: cargo run --example game_server
use mycelium_protocol::{impl_message, Message};
use mycelium_transport::{BoundedPublisher, BoundedSubscriber, MessageBus};
use zerocopy::{AsBytes, FromBytes, FromZeroes};

/// Player action (movement, attack, etc.)
#[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
struct PlayerAction {
    player_id: u64,
    action_type: u64, // 1=move, 2=attack, 3=use_item (u64 to avoid padding)
    target_x: i64,    // i64 to avoid padding
    target_y: i64,    // i64 to avoid padding
    timestamp_ms: u64,
}

impl_message!(PlayerAction, 10, "game.actions");

/// Game state update (broadcast to all players)
#[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
struct StateUpdate {
    player_id: u64,
    position_x: i64, // i64 to avoid padding
    position_y: i64, // i64 to avoid padding
    health: u64,     // u64 to avoid padding
    tick: u64,
}

impl_message!(StateUpdate, 11, "game.state");

/// Chat message
#[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
struct ChatMessage {
    player_id: u64,
    message_len: u64, // u64 to avoid padding
    message_id: u64,
}

impl_message!(ChatMessage, 12, "game.chat");

#[tokio::main]
async fn main() {
    println!("=== Mycelium: Generic Game Server Example ===\n");

    let bus = MessageBus::new();

    // Player action channel (with backpressure to prevent spam)
    let (action_pub, mut action_recv): (
        BoundedPublisher<PlayerAction>,
        BoundedSubscriber<PlayerAction>,
    ) = bus.bounded_pair(100);

    // State updates (broadcast to all clients)
    let state_pub = bus.publisher::<StateUpdate>();
    let mut state_sub1 = bus.subscriber::<StateUpdate>();
    let mut state_sub2 = bus.subscriber::<StateUpdate>();

    // Chat (broadcast)
    let chat_pub = bus.publisher::<ChatMessage>();
    let mut chat_sub = bus.subscriber::<ChatMessage>();

    println!("🎮 Game server initialized");
    println!("   - Action channel (bounded, with backpressure)");
    println!("   - State broadcast (all players)");
    println!("   - Chat broadcast\n");

    // Game logic processor (processes actions and publishes state updates)
    tokio::spawn(async move {
        println!("🎯 [Server] Game logic processor started\n");

        while let Some(action) = action_recv.recv().await {
            let action_name = match action.action_type {
                1 => "MOVE",
                2 => "ATTACK",
                3 => "USE_ITEM",
                _ => "UNKNOWN",
            };

            println!(
                "⚙️  [Server] Processing: Player {} {} to ({}, {})",
                action.player_id, action_name, action.target_x, action.target_y
            );

            // Simulate processing and publish state update
            let state = StateUpdate {
                player_id: action.player_id,
                position_x: action.target_x,
                position_y: action.target_y,
                health: 100,
                tick: action.timestamp_ms / 16, // ~60 FPS
            };

            state_pub.publish(state).await.ok();
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
    });

    // Player 1 client (receives state updates)
    tokio::spawn(async move {
        while let Some(state) = state_sub1.recv().await {
            println!(
                "📺 [Player 1 Client] State update: Player {} at ({}, {}) HP:{}",
                state.player_id, state.position_x, state.position_y, state.health
            );
        }
    });

    // Player 2 client (receives state updates)
    tokio::spawn(async move {
        while let Some(state) = state_sub2.recv().await {
            println!(
                "📺 [Player 2 Client] State update: Player {} at ({}, {}) HP:{}",
                state.player_id, state.position_x, state.position_y, state.health
            );
        }
    });

    // Chat listener
    tokio::spawn(async move {
        while let Some(msg) = chat_sub.recv().await {
            println!(
                "💬 [Chat] Player {} sent message #{}",
                msg.player_id, msg.message_id
            );
        }
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Simulate player actions
    println!("🎬 Simulating player actions...\n");

    let actions = vec![
        (1001, 1, 100, 200), // Player 1001 moves
        (1002, 1, 150, 180), // Player 1002 moves
        (1001, 2, 150, 180), // Player 1001 attacks player 1002's position
        (1002, 3, 150, 180), // Player 1002 uses item
        (1001, 1, 175, 190), // Player 1001 moves
    ];

    for (player_id, action_type, x, y) in actions {
        let action = PlayerAction {
            player_id,
            action_type,
            target_x: x,
            target_y: y,
            timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        };

        action_pub.publish(action).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
    }

    // Simulate chat
    let chat = ChatMessage {
        player_id: 1001,
        message_len: 5,
        message_id: 42,
    };
    chat_pub.publish(chat).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    println!("\n=== Example Complete ===");
    println!("✨ Generic pub/sub - no domain-specific terminology");
    println!("   Demonstrates:");
    println!("   - Bounded channels for backpressure (prevents action spam)");
    println!("   - Broadcast for state synchronization");
    println!("   - Low-latency message passing (<1ms local)");
    println!("\n   Works for: FPS, MMO, RTS, racing games, etc.");
}
