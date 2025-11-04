//! Actor system demonstration
//!
//! Shows how to use the actor system for building stateful services
//! that communicate via message passing.

use mycelium_protocol::impl_message;
use mycelium_transport::{Actor, ActorContext, ActorRuntime, MessageBus};
use zerocopy::{IntoBytes, FromBytes, FromZeros, Immutable};

// Define message types
#[derive(Debug, Clone, Copy, PartialEq, IntoBytes, FromBytes, FromZeros, Immutable)]
#[repr(C)]
struct Increment {
    amount: u64,
}

impl_message!(Increment, 100, "counter");

#[derive(Debug, Clone, Copy, PartialEq, IntoBytes, FromBytes, FromZeros, Immutable)]
#[repr(C)]
struct GetCount {
    _dummy: u8, // Empty structs not supported by zerocopy
}

impl_message!(GetCount, 101, "counter.query");

// Counter actor that maintains state
struct CounterActor {
    count: u64,
    name: String,
}

#[async_trait::async_trait]
impl Actor for CounterActor {
    type Message = Increment;

    async fn handle(&mut self, msg: Self::Message, _ctx: &mut ActorContext<Self>) {
        self.count += msg.amount;
        println!("[{}] Count increased by {}. New total: {}", self.name, msg.amount, self.count);
    }

    async fn started(&mut self, ctx: &mut ActorContext<Self>) {
        println!("[{}] Actor started with ID: {}", self.name, ctx.actor_id());
    }

    async fn stopped(&mut self, _ctx: &mut ActorContext<Self>) {
        println!("[{}] Actor stopped. Final count: {}", self.name, self.count);
    }
}

#[tokio::main]
async fn main() {
    // Initialize runtime
    let bus = MessageBus::new();
    let runtime = ActorRuntime::new(bus);

    println!("=== Actor System Demo ===\n");

    // Spawn multiple counter actors
    let counter1 = runtime.spawn(CounterActor {
        count: 0,
        name: "Counter1".to_string(),
    }).await;

    let counter2 = runtime.spawn(CounterActor {
        count: 100,
        name: "Counter2".to_string(),
    }).await;

    println!("\n--- Sending Messages ---\n");

    // Send messages to actors
    counter1.send(Increment { amount: 10 }).await.unwrap();
    counter1.send(Increment { amount: 5 }).await.unwrap();

    counter2.send(Increment { amount: 20 }).await.unwrap();

    // Give actors time to process
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    println!("\n--- Demonstration Complete ---");
    println!("Actor count: {}", runtime.actor_count().await);

    // Graceful shutdown
    println!("\nShutting down...");
    runtime.shutdown().await;
}
