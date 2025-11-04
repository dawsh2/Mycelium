//! Actor system built on top of pub/sub messaging
//!
//! This module provides a lightweight actor abstraction where:
//! - Actors are pub/sub subscribers with dedicated mailbox topics
//! - Messages are delivered via the same transport layer (Arc, Unix, TCP)
//! - Actor lifecycle is managed by ActorRuntime
//! - Request/reply is supported via correlation IDs
//!
//! # Example
//!
//! ```rust,ignore
//! use crate::{Actor, ActorContext, ActorRuntime, MessageBus};
//!
//! struct MyActor {
//!     state: u64,
//! }
//!
//! #[async_trait::async_trait]
//! impl Actor for MyActor {
//!     type Message = MyMessage;
//!
//!     async fn handle(&mut self, msg: Self::Message, ctx: &mut ActorContext<Self>) {
//!         self.state += msg.value;
//!         println!("State: {}", self.state);
//!     }
//! }
//!
//! let bus = MessageBus::new();
//! let runtime = ActorRuntime::new(bus);
//! let actor_ref = runtime.spawn(MyActor { state: 0 }).await;
//! actor_ref.send(MyMessage { value: 42 }).await;
//! ```

mod context;
mod handle;
mod runtime;
mod supervisor;

pub use context::{ActorContext, ActorRef};
pub use handle::Actor;
pub use runtime::{ActorRuntime, SpawnError};
pub use supervisor::{SupervisionStrategy, RestartStrategy};
