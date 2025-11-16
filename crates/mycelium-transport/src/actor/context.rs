//! Actor context and reference types
//!
//! Provides ActorContext for use within actors and ActorRef for sending messages.

use crate::{MessageBus, Publisher, Result, TransportError};
use mycelium_protocol::routing::{ActorId, CorrelationId};
use mycelium_protocol::Message;

/// Context provided to actors during message handling
///
/// Allows actors to:
/// - Send messages to other actors
/// - Access their own ActorId
/// - Stop themselves
/// - Send request/reply messages
pub struct ActorContext<A: super::Actor> {
    /// This actor's unique ID
    actor_id: ActorId,
    /// Message bus for publishing messages
    bus: MessageBus,
    /// Flag to signal actor should stop
    should_stop: bool,
    /// Phantom data for actor type
    _phantom: std::marker::PhantomData<A>,
}

impl<A: super::Actor> ActorContext<A> {
    /// Create a new actor context
    pub(crate) fn new(actor_id: ActorId, bus: MessageBus) -> Self {
        Self {
            actor_id,
            bus,
            should_stop: false,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Get this actor's ID
    pub fn actor_id(&self) -> ActorId {
        self.actor_id
    }

    /// Send a message to another actor
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// ctx.send_to(other_actor_id, MyMessage { value: 42 }).await?;
    /// ```
    pub async fn send_to<M: Message>(&self, target: ActorId, msg: M) -> Result<()> {
        let topic = crate::TopicBuilder::actor_mailbox(target);
        let publisher = self.bus.publisher_for_topic::<M>(&topic);

        // Wrap in envelope with destination
        publisher.publish(msg).await
    }

    /// Broadcast a message to all subscribers on a topic
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// ctx.broadcast(MyMessage { value: 42 }).await?;
    /// ```
    pub async fn broadcast<M: Message>(&self, msg: M) -> Result<()> {
        let publisher = self.bus.publisher::<M>();
        publisher.publish(msg).await
    }

    /// Send a request and wait for a reply
    ///
    /// This is a convenience method that:
    /// 1. Generates a correlation ID
    /// 2. Sends the request with that correlation ID
    /// 3. Waits for a reply with the matching correlation ID
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let reply = ctx.request::<MyRequest, MyReply>(
    ///     other_actor_id,
    ///     MyRequest { query: "status" }
    /// ).await?;
    /// ```
    pub async fn request<Req, Rep>(&self, _target: ActorId, _request: Req) -> Result<Rep>
    where
        Req: Message,
        Rep: Message,
    {
        Err(TransportError::InvalidConfig(
            "ActorContext::request is not implemented. Use send()/reply patterns manually."
                .to_string(),
        ))
    }

    /// Stop this actor
    ///
    /// The actor will finish processing the current message and then shut down.
    pub fn stop(&mut self) {
        self.should_stop = true;
    }

    /// Check if the actor should stop
    pub(crate) fn should_stop(&self) -> bool {
        self.should_stop
    }

    /// Get a reference to the message bus
    pub fn bus(&self) -> &MessageBus {
        &self.bus
    }
}

/// Reference to an actor for sending messages
///
/// This is a lightweight handle that can be cloned and shared.
/// It allows sending messages to the actor's mailbox.
#[derive(Clone)]
pub struct ActorRef<M: Message> {
    actor_id: ActorId,
    publisher: Publisher<M>,
}

impl<M: Message> ActorRef<M> {
    /// Create a new actor reference
    pub(crate) fn new(actor_id: ActorId, publisher: Publisher<M>) -> Self {
        Self {
            actor_id,
            publisher,
        }
    }

    /// Get the actor's ID
    pub fn actor_id(&self) -> ActorId {
        self.actor_id
    }

    /// Send a message to this actor
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// actor_ref.send(MyMessage { value: 42 }).await?;
    /// ```
    pub async fn send(&self, msg: M) -> Result<()> {
        self.publisher.publish(msg).await
    }

    /// Send a message with a correlation ID (for request/reply)
    ///
    /// NOTE: Correlation tracking is not yet implemented. This currently sends
    /// the message without correlation metadata. Future implementation will wrap
    /// the message in an envelope with routing information.
    pub async fn send_with_correlation(
        &self,
        _msg: M,
        _correlation_id: CorrelationId,
    ) -> Result<()> {
        Err(TransportError::InvalidConfig(
            "Correlation-aware actor messaging is not implemented yet".to_string(),
        ))
    }
}

impl<M: Message> std::fmt::Debug for ActorRef<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActorRef")
            .field("actor_id", &self.actor_id)
            .finish()
    }
}
