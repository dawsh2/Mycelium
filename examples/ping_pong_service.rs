//! Ping-Pong Service Example
//!
//! Demonstrates the symmetric ctx.subscribe() and ctx.emit() API.
//! Services can now both send and receive messages through ServiceContext.

use anyhow::Result;
use mycelium_protocol::impl_message;
use mycelium_transport::{service, MessageBus, ServiceContext, ServiceRuntime};
use std::time::Duration;
use zerocopy::{IntoBytes, FromBytes, FromZeros, Immutable};

/// Ping message
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, FromZeros, Immutable)]
#[repr(C)]
struct PingMessage {
    id: u64,
}

impl_message!(PingMessage, 100, "ping");

/// Pong message
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, FromZeros, Immutable)]
#[repr(C)]
struct PongMessage {
    id: u64,
}

impl_message!(PongMessage, 101, "pong");

/// PongService responds to ping messages with pong messages
struct PongService;

#[service]
impl PongService {
    async fn run(&mut self, ctx: &ServiceContext) -> Result<()> {
        ctx.info("PongService starting - waiting for pings");

        // Subscribe to pings directly from context (symmetric with emit!)
        let mut ping_sub = ctx.subscribe::<PingMessage>();

        // Respond to pings with pongs
        while let Some(ping) = ping_sub.recv().await {
            ctx.info(&format!("Received ping #{}", ping.id));

            let pong = PongMessage { id: ping.id };
            ctx.emit(pong).await?;

            ctx.info(&format!("Sent pong #{}", pong.id));

            // Exit after 3 pongs
            if ping.id >= 3 {
                ctx.info("Received 3 pings, exiting");
                break;
            }
        }

        Ok(())
    }
}

/// PingService sends ping messages and waits for pong responses
struct PingService;

#[service]
impl PingService {
    async fn run(&mut self, ctx: &ServiceContext) -> Result<()> {
        ctx.info("PingService starting");

        // Subscribe to pongs directly from context
        let mut pong_sub = ctx.subscribe::<PongMessage>();

        // Send 3 pings
        for i in 1..=3 {
            ctx.info(&format!("Sending ping #{}", i));

            let ping = PingMessage { id: i };
            ctx.emit(ping).await?;

            // Wait for pong response
            if let Some(pong) = pong_sub.recv().await {
                ctx.info(&format!("Received pong #{}", pong.id));
            }

            ctx.sleep(Duration::from_millis(500)).await;
        }

        ctx.info("Sent all pings, exiting");
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Ping-Pong Service Example ===");
    println!("Demonstrates symmetric ctx.subscribe() and ctx.emit() API\n");

    let bus = MessageBus::new();
    let runtime = ServiceRuntime::new(bus);

    // Spawn both services
    let pong_handle = runtime.spawn_service(PongService).await?;
    let ping_handle = runtime.spawn_service(PingService).await?;

    println!("Services running...\n");

    // Wait for both to complete
    let _ = tokio::join!(pong_handle.join(), ping_handle.join());

    println!("\n=== Example Complete ===");
    println!("Services used ctx.subscribe() to receive messages");
    println!("and ctx.emit() to send messages - symmetric API!");

    Ok(())
}
