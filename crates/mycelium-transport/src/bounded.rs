/// Bounded publisher with backpressure support
///
/// Uses `tokio::sync::mpsc` bounded channels to provide natural backpressure.
/// When the channel is full, publishers block until space is available.
///
/// ## Design Philosophy (from ACTORS.md)
///
/// - Actor mailboxes provide bounded capacity with backpressure
/// - Pub/sub can achieve backpressure via bounded channels
/// - BoundedPublisher trades broadcast capability for flow control
/// - Use when you need to prevent fast publishers from overwhelming subscribers
///
/// ## Tradeoffs vs Regular Publisher
///
/// | Feature | Publisher (broadcast) | BoundedPublisher (mpsc) |
/// |---------|----------------------|-------------------------|
/// | Multiple subscribers | ✅ Yes (fan-out) | ❌ No (single consumer) |
/// | Backpressure | ❌ Lagging drops messages | ✅ Blocks when full |
/// | Use case | Market data broadcast | Actor mailboxes, point-to-point |
/// | Overhead | Low | Slightly higher |
///
/// ## Use Cases
///
/// - Actor mailboxes (single consumer with backpressure)
/// - Point-to-point messaging where backpressure is needed
/// - Rate-limiting message flows
/// - Preventing memory exhaustion from fast producers
use crate::error::Result;
use mycelium_protocol::{Envelope, Message};
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Publisher with bounded capacity and backpressure
///
/// Unlike the standard `Publisher` which uses broadcast channels,
/// `BoundedPublisher` uses bounded mpsc channels. This provides:
/// - Natural backpressure when the channel is full
/// - Single-consumer semantics (actor mailbox pattern)
/// - Prevention of memory exhaustion
///
/// # Example
///
/// ```rust,no_run
/// use mycelium_transport::{BoundedPublisher, BoundedSubscriber};
/// # use mycelium_protocol::PoolStateUpdate;
///
/// # async fn example() {
/// // Create bounded channel with capacity 100
/// let (pub_, mut sub) = BoundedPublisher::<PoolStateUpdate>::new(100);
///
/// // Publish message (blocks if channel is full)
/// pub_.publish(PoolStateUpdate::default()).await.unwrap();
///
/// // Receive message
/// if let Some(msg) = sub.recv().await {
///     println!("Received: {:?}", msg);
/// }
/// # }
/// ```
pub struct BoundedPublisher<M: Message> {
    tx: mpsc::Sender<Envelope>,
    capacity: usize,
    _phantom: PhantomData<M>,
}

impl<M: Message> BoundedPublisher<M> {
    /// Create a new bounded publisher-subscriber pair
    ///
    /// # Arguments
    ///
    /// * `capacity` - Maximum number of messages that can be buffered
    ///
    /// # Example
    ///
    /// ```rust
    /// # use mycelium_transport::BoundedPublisher;
    /// # use mycelium_protocol::PoolStateUpdate;
    /// let (publisher, subscriber) = BoundedPublisher::<PoolStateUpdate>::new(100);
    /// ```
    pub fn new(capacity: usize) -> (Self, BoundedSubscriber<M>) {
        let (tx, rx) = mpsc::channel(capacity);

        let publisher = Self {
            tx,
            capacity,
            _phantom: PhantomData,
        };

        let subscriber = BoundedSubscriber {
            rx,
            _phantom: PhantomData,
        };

        (publisher, subscriber)
    }

    /// Publish a message (async, with backpressure)
    ///
    /// This method will block if the channel is full, providing natural backpressure.
    /// The publisher will wait until the subscriber consumes enough messages to free space.
    ///
    /// Returns `Err` if the receiver has been dropped.
    pub async fn publish(&self, msg: M) -> Result<()> {
        let envelope = Envelope::new(msg);
        self.tx
            .send(envelope)
            .await
            .map_err(|_| crate::TransportError::ChannelClosed)?;
        Ok(())
    }

    /// Try to publish a message (non-blocking)
    ///
    /// Returns immediately. If the channel is full, returns an error.
    /// Use this for fire-and-forget scenarios where you want to detect backpressure.
    pub fn try_publish(&self, msg: M) -> Result<()> {
        let envelope = Envelope::new(msg);
        self.tx
            .try_send(envelope)
            .map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => crate::TransportError::ChannelFull,
                mpsc::error::TrySendError::Closed(_) => crate::TransportError::ChannelClosed,
            })?;
        Ok(())
    }

    /// Get the channel capacity
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Get the current number of messages in the channel
    ///
    /// Note: This is a snapshot and may change immediately after reading.
    pub fn len(&self) -> usize {
        // mpsc doesn't expose current length, so we estimate via capacity
        // In a real implementation, we'd need to track this separately
        0 // TODO: Track actual length
    }

    /// Check if the channel is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Check if the channel is full (would block on publish)
    pub fn is_full(&self) -> bool {
        // Would need custom tracking to implement accurately
        false // TODO: Track fullness
    }

    /// Check if the receiver is still alive
    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }
}

impl<M: Message> Clone for BoundedPublisher<M> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            capacity: self.capacity,
            _phantom: PhantomData,
        }
    }
}

/// Subscriber for bounded channels
///
/// Receives messages from a `BoundedPublisher`. Unlike broadcast subscribers,
/// only one subscriber can exist per publisher (single-consumer pattern).
pub struct BoundedSubscriber<M: Message> {
    rx: mpsc::Receiver<Envelope>,
    _phantom: PhantomData<M>,
}

impl<M: Message> BoundedSubscriber<M> {
    /// Receive the next message
    ///
    /// Blocks until a message is available or the channel is closed.
    /// Returns `None` if the publisher has been dropped and the channel is empty.
    pub async fn recv(&mut self) -> Option<Arc<M>> {
        loop {
            let envelope = self.rx.recv().await?;

            // Filter by type ID (in case of mixed message types)
            if envelope.type_id != M::TYPE_ID {
                tracing::warn!(
                    "Received message with wrong type ID: expected {}, got {}",
                    M::TYPE_ID,
                    envelope.type_id
                );
                continue;
            }

            // Downcast to concrete type
            match envelope.downcast::<M>() {
                Ok(msg) => return Some(msg),
                Err(e) => {
                    tracing::error!("Failed to downcast message: {}", e);
                    continue;
                }
            }
        }
    }

    /// Try to receive a message (non-blocking)
    ///
    /// Returns immediately. If no message is available, returns `None`.
    pub fn try_recv(&mut self) -> Option<Arc<M>> {
        loop {
            let envelope = match self.rx.try_recv() {
                Ok(env) => env,
                Err(mpsc::error::TryRecvError::Empty) => return None,
                Err(mpsc::error::TryRecvError::Disconnected) => return None,
            };

            // Filter by type ID
            if envelope.type_id != M::TYPE_ID {
                continue;
            }

            // Downcast
            match envelope.downcast::<M>() {
                Ok(msg) => return Some(msg),
                Err(e) => {
                    tracing::error!("Failed to downcast message: {}", e);
                    continue;
                }
            }
        }
    }

    /// Close the receiver, preventing any more messages from being received
    pub fn close(&mut self) {
        self.rx.close();
    }
}

/// Builder for creating bounded publishers with custom configuration
///
/// Provides a fluent API for configuring bounded channels.
///
/// # Example
///
/// ```rust
/// # use mycelium_transport::BoundedPublisherBuilder;
/// # use mycelium_protocol::PoolStateUpdate;
/// let (publisher, subscriber) = BoundedPublisherBuilder::<PoolStateUpdate>::new()
///     .capacity(1000)
///     .build();
/// ```
pub struct BoundedPublisherBuilder<M: Message> {
    capacity: usize,
    _phantom: PhantomData<M>,
}

impl<M: Message> BoundedPublisherBuilder<M> {
    /// Create a new builder with default capacity (100)
    pub fn new() -> Self {
        Self {
            capacity: 100,
            _phantom: PhantomData,
        }
    }

    /// Set the channel capacity
    pub fn capacity(mut self, capacity: usize) -> Self {
        self.capacity = capacity;
        self
    }

    /// Build the publisher-subscriber pair
    pub fn build(self) -> (BoundedPublisher<M>, BoundedSubscriber<M>) {
        BoundedPublisher::new(self.capacity)
    }
}

impl<M: Message> Default for BoundedPublisherBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mycelium_protocol::impl_message;
    use zerocopy::{AsBytes, FromBytes, FromZeroes};

    #[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
    #[repr(C)]
    struct TestMsg {
        value: u64,
    }

    impl_message!(TestMsg, 1, "test");

    #[tokio::test]
    async fn test_bounded_publisher_basic() {
        let (pub_, mut sub) = BoundedPublisher::<TestMsg>::new(10);

        pub_.publish(TestMsg { value: 42 }).await.unwrap();

        let msg = sub.recv().await.unwrap();
        assert_eq!(msg.value, 42);
    }

    #[tokio::test]
    async fn test_bounded_publisher_multiple_messages() {
        let (pub_, mut sub) = BoundedPublisher::<TestMsg>::new(10);

        // Send multiple messages
        for i in 0..5 {
            pub_.publish(TestMsg { value: i }).await.unwrap();
        }

        // Receive them
        for i in 0..5 {
            let msg = sub.recv().await.unwrap();
            assert_eq!(msg.value, i);
        }
    }

    #[tokio::test]
    async fn test_bounded_publisher_backpressure() {
        let (pub_, mut sub) = BoundedPublisher::<TestMsg>::new(2);

        // Fill the channel (capacity = 2)
        pub_.publish(TestMsg { value: 1 }).await.unwrap();
        pub_.publish(TestMsg { value: 2 }).await.unwrap();

        // Next publish should block until we receive
        let pub_clone = pub_.clone();
        let publish_task = tokio::spawn(async move {
            pub_clone.publish(TestMsg { value: 3 }).await.unwrap();
        });

        // Give time for the task to start and block
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Now receive one message to free space
        let msg1 = sub.recv().await.unwrap();
        assert_eq!(msg1.value, 1);

        // The blocked publish should now complete
        publish_task.await.unwrap();

        // Receive remaining messages
        let msg2 = sub.recv().await.unwrap();
        assert_eq!(msg2.value, 2);

        let msg3 = sub.recv().await.unwrap();
        assert_eq!(msg3.value, 3);
    }

    #[tokio::test]
    async fn test_bounded_publisher_try_publish() {
        let (pub_, mut sub) = BoundedPublisher::<TestMsg>::new(1);

        // First publish should succeed
        pub_.try_publish(TestMsg { value: 1 }).unwrap();

        // Second should fail (channel full)
        let result = pub_.try_publish(TestMsg { value: 2 });
        assert!(result.is_err());

        // Receive to free space
        sub.recv().await.unwrap();

        // Now it should succeed
        pub_.try_publish(TestMsg { value: 2 }).unwrap();
    }

    #[tokio::test]
    async fn test_bounded_publisher_closed() {
        let (pub_, sub) = BoundedPublisher::<TestMsg>::new(10);

        assert!(!pub_.is_closed());

        // Drop subscriber
        drop(sub);

        assert!(pub_.is_closed());

        // Publishing should fail
        let result = pub_.publish(TestMsg { value: 1 }).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_bounded_publisher_builder() {
        let (pub_, _sub) = BoundedPublisherBuilder::<TestMsg>::new()
            .capacity(500)
            .build();

        assert_eq!(pub_.capacity(), 500);
    }

    #[tokio::test]
    async fn test_bounded_subscriber_try_recv() {
        let (pub_, mut sub) = BoundedPublisher::<TestMsg>::new(10);

        // No message available
        assert!(sub.try_recv().is_none());

        // Send message
        pub_.publish(TestMsg { value: 42 }).await.unwrap();

        // Now try_recv should get it
        let msg = sub.try_recv().unwrap();
        assert_eq!(msg.value, 42);

        // Should be empty again
        assert!(sub.try_recv().is_none());
    }
}
