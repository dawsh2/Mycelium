//! Actor trait and handler pattern
//!
//! Defines the core Actor trait that users implement for their actor logic.

use super::context::ActorContext;
use mycelium_protocol::Message;

/// Core actor trait
///
/// Implement this trait to create an actor. The actor processes messages
/// via the `handle` method, which receives a message and a context for
/// interacting with the actor system.
///
/// # Example
///
/// ```rust,ignore
/// use crate::{Actor, ActorContext};
///
/// struct CounterActor {
///     count: u64,
/// }
///
/// #[async_trait::async_trait]
/// impl Actor for CounterActor {
///     type Message = CounterMessage;
///
///     async fn handle(&mut self, msg: Self::Message, ctx: &mut ActorContext<Self>) {
///         match msg {
///             CounterMessage::Increment => self.count += 1,
///             CounterMessage::GetCount => {
///                 println!("Count: {}", self.count);
///             }
///         }
///     }
///
///     async fn started(&mut self, ctx: &mut ActorContext<Self>) {
///         println!("Actor started with ID: {}", ctx.actor_id());
///     }
///
///     async fn stopped(&mut self, ctx: &mut ActorContext<Self>) {
///         println!("Actor stopped. Final count: {}", self.count);
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait Actor: Send + Sized + 'static {
    /// The message type this actor handles
    type Message: Message;

    /// Handle a message
    ///
    /// This is the core method where the actor's business logic lives.
    /// It receives a message and a context for sending messages to other actors.
    async fn handle(&mut self, msg: Self::Message, ctx: &mut ActorContext<Self>);

    /// Called when the actor is started (before processing messages)
    ///
    /// Override this to perform initialization logic.
    async fn started(&mut self, _ctx: &mut ActorContext<Self>) {
        // Default: do nothing
    }

    /// Called when the actor is stopped (after processing all messages)
    ///
    /// Override this to perform cleanup logic.
    async fn stopped(&mut self, _ctx: &mut ActorContext<Self>) {
        // Default: do nothing
    }

    /// Called when an error occurs in the actor's message handler
    ///
    /// Override this to customize error handling. Return `true` to continue
    /// processing messages, or `false` to stop the actor.
    ///
    /// Default behavior: log the error and continue.
    async fn on_error(
        &mut self,
        error: Box<dyn std::error::Error + Send>,
        _ctx: &mut ActorContext<Self>,
    ) -> bool {
        tracing::error!("Actor error: {}", error);
        true // Continue processing
    }
}
