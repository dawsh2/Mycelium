//! Actor context and reference types
//!
//! Provides ActorContext for use within actors and ActorRef for sending messages.

use crate::{MessageBus, Publisher, Result};
use mycelium_protocol::routing::{ActorId, CorrelationId};
use mycelium_protocol::Message;
use tokio::sync::oneshot;

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
    pub async fn request<Req, Rep>(
        &self,
        target: ActorId,
        request: Req,
    ) -> Result<Rep>
    where
        Req: Message,
        Rep: Message,
    {
        // Generate correlation ID
        let _correlation_id = CorrelationId::new();

        // Create a oneshot channel for the reply
        let (_tx, rx) = oneshot::channel();

        // TODO: Register reply handler with correlation_id -> tx mapping
        // For now, this is a placeholder implementation
        // Full implementation would require a registry in the runtime

        // Send request with correlation ID
        let topic = crate::TopicBuilder::actor_mailbox(target);
        let publisher = self.bus.publisher_for_topic::<Req>(&topic);

        // TODO: Wrap in envelope with correlation_id
        publisher.publish(request).await?;

        // Wait for reply (with timeout)
        let reply = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            rx
        ).await
            .map_err(|_| crate::TransportError::SendFailed)?
            .map_err(|_| crate::TransportError::SendFailed)?;

        Ok(reply)
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
    pub async fn send_with_correlation(&self, msg: M, _correlation_id: CorrelationId) -> Result<()> {
        // TODO: Wrap in envelope with correlation_id
        self.publisher.publish(msg).await
    }
}

impl<M: Message> std::fmt::Debug for ActorRef<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActorRef")
            .field("actor_id", &self.actor_id)
            .finish()
    }
}
