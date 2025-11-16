use mycelium_protocol::{Envelope, Message};
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Subscriber for a specific message type
///
/// Subscribers receive messages published to their topic.
/// Automatically filters messages by type ID.
pub struct Subscriber<M: Message> {
    rx: broadcast::Receiver<Envelope>,
    _phantom: PhantomData<M>,
}

impl<M: Message> Subscriber<M> {
    pub(crate) fn new(rx: broadcast::Receiver<Envelope>) -> Self {
        Self {
            rx,
            _phantom: PhantomData,
        }
    }

    /// Receive the next message
    ///
    /// This method filters out messages with different type IDs
    /// and only returns messages of type M.
    ///
    /// Returns None if the channel is closed.
    pub async fn recv(&mut self) -> Option<Arc<M>> {
        loop {
            // Receive envelope from broadcast channel
            let envelope = match self.rx.recv().await {
                Ok(env) => env,
                Err(broadcast::error::RecvError::Closed) => return None,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("Subscriber lagged by {} messages", n);
                    continue;
                }
            };

            // Filter by type ID
            if envelope.type_id != M::TYPE_ID {
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

    /// Receive the next message with its sequence number
    ///
    /// This method is similar to `recv()` but also returns the sequence number
    /// from the envelope metadata. Used by OrderedSubscriber for message reordering.
    ///
    /// Returns None if the channel is closed.
    pub async fn recv_with_sequence(&mut self) -> Option<(Arc<M>, Option<u64>)> {
        loop {
            // Receive envelope from broadcast channel
            let envelope = match self.rx.recv().await {
                Ok(env) => env,
                Err(broadcast::error::RecvError::Closed) => return None,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("Subscriber lagged by {} messages", n);
                    continue;
                }
            };

            // Filter by type ID
            if envelope.type_id != M::TYPE_ID {
                continue;
            }

            // Extract sequence before downcast
            let sequence = envelope.sequence;

            // Downcast to concrete type
            match envelope.downcast::<M>() {
                Ok(msg) => return Some((msg, sequence)),
                Err(e) => {
                    tracing::error!("Failed to downcast message: {}", e);
                    continue;
                }
            }
        }
    }

    /// Try to receive a message (non-blocking)
    ///
    /// Returns immediately with None if no message is available.
    pub fn try_recv(&mut self) -> Option<Arc<M>> {
        loop {
            let envelope = match self.rx.try_recv() {
                Ok(env) => env,
                Err(broadcast::error::TryRecvError::Empty) => return None,
                Err(broadcast::error::TryRecvError::Closed) => return None,
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    tracing::warn!("Subscriber lagged by {} messages", n);
                    continue;
                }
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

    #[derive(Debug, Clone, Copy, AsBytes, FromBytes, FromZeroes)]
    #[repr(C)]
    struct OtherMsg {
        data: u64,
    }

    impl_message!(OtherMsg, 2, "other");

    #[tokio::test]
    async fn test_subscriber_recv() {
        let (tx, rx) = broadcast::channel(10);
        let mut sub: Subscriber<TestMsg> = Subscriber::new(rx);

        // Send message
        let envelope = Envelope::new(TestMsg { value: 42 });
        tx.send(envelope).unwrap();

        // Receive message
        let msg = sub.recv().await.unwrap();
        assert_eq!(msg.value, 42);
    }

    #[tokio::test]
    async fn test_subscriber_filters_by_type() {
        let (tx, rx) = broadcast::channel(10);
        let mut sub: Subscriber<TestMsg> = Subscriber::new(rx);

        // Send wrong type
        let wrong_envelope = Envelope::new(OtherMsg { data: 999 });
        tx.send(wrong_envelope).unwrap();

        // Send correct type
        let correct_envelope = Envelope::new(TestMsg { value: 42 });
        tx.send(correct_envelope).unwrap();

        // Should receive only correct type
        let msg = sub.recv().await.unwrap();
        assert_eq!(msg.value, 42);
    }

    #[tokio::test]
    async fn test_subscriber_channel_closed() {
        let (tx, rx) = broadcast::channel::<Envelope>(10);
        let mut sub: Subscriber<TestMsg> = Subscriber::new(rx);

        drop(tx);

        // Should return None when channel is closed
        let result = sub.recv().await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_try_recv_empty() {
        let (_tx, rx) = broadcast::channel(10);
        let mut sub: Subscriber<TestMsg> = Subscriber::new(rx);

        // Should return None immediately when empty
        let result = sub.try_recv();
        assert!(result.is_none());
    }
}
