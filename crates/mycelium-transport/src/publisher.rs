use crate::error::Result;
use mycelium_protocol::{Envelope, Message};
use std::marker::PhantomData;
use tokio::sync::broadcast;

/// Publisher for a specific message type
///
/// Publishers send messages to all subscribers of the same topic.
/// Uses broadcast channels for efficient multi-subscriber delivery.
pub struct Publisher<M: Message> {
    tx: broadcast::Sender<Envelope>,
    _phantom: PhantomData<M>,
}

impl<M: Message> Publisher<M> {
    pub(crate) fn new(tx: broadcast::Sender<Envelope>) -> Self {
        Self {
            tx,
            _phantom: PhantomData,
        }
    }

    /// Publish a message to all subscribers
    ///
    /// Returns an error if there are no active subscribers.
    /// This fail-fast behavior prevents silent message loss in production systems.
    ///
    /// For systems where no-subscriber scenarios are expected, use `try_publish_lossy`
    /// which logs a warning instead of returning an error.
    pub async fn publish(&self, msg: M) -> Result<()> {
        let envelope = Envelope::new(msg);
        let receiver_count = self
            .tx
            .send(envelope)
            .map_err(|_| crate::TransportError::SendFailed)?;

        if receiver_count == 0 {
            return Err(crate::TransportError::NoSubscribers {
                topic: M::TOPIC.to_string(),
            });
        }

        Ok(())
    }

    /// Try to publish a message (non-blocking)
    ///
    /// Returns an error if there are no active subscribers (same as publish).
    pub fn try_publish(&self, msg: M) -> Result<()> {
        let envelope = Envelope::new(msg);
        let receiver_count = self
            .tx
            .send(envelope)
            .map_err(|_| crate::TransportError::SendFailed)?;

        if receiver_count == 0 {
            return Err(crate::TransportError::NoSubscribers {
                topic: M::TOPIC.to_string(),
            });
        }

        Ok(())
    }

    /// Publish a message with lossy semantics (logs warning on no subscribers)
    ///
    /// Use this variant when publishing to optional subscribers where
    /// message loss is acceptable (e.g., metrics, debug telemetry).
    ///
    /// For critical messages, use `publish()` which fails if no subscribers exist.
    pub async fn publish_lossy(&self, msg: M) -> Result<()> {
        let envelope = Envelope::new(msg);
        let receiver_count = self
            .tx
            .send(envelope)
            .map_err(|_| crate::TransportError::SendFailed)?;

        if receiver_count == 0 {
            tracing::warn!(
                topic = M::TOPIC,
                "Published message but no active subscribers - message dropped"
            );
        }

        Ok(())
    }

    /// Get the number of active subscribers
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl<M: Message> Clone for Publisher<M> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            _phantom: PhantomData,
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

    #[tokio::test]
    async fn test_publisher_no_subscribers() {
        let (tx, rx) = broadcast::channel(10);
        let pub_: Publisher<TestMsg> = Publisher::new(tx.clone());

        // Drop the receiver so there are no subscribers
        drop(rx);

        // Check that there are no subscribers
        assert_eq!(pub_.subscriber_count(), 0);

        // Publishing with no subscribers should fail (critical for trading systems)
        let result = pub_.publish(TestMsg { value: 42 }).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_publisher_with_subscriber() {
        let (tx, mut rx) = broadcast::channel(10);
        let pub_: Publisher<TestMsg> = Publisher::new(tx);

        pub_.publish(TestMsg { value: 42 }).await.unwrap();

        let envelope = rx.recv().await.unwrap();
        assert_eq!(envelope.type_id, 1);
    }

    #[tokio::test]
    async fn test_publisher_clone() {
        let (tx, mut rx) = broadcast::channel(10);
        let pub1: Publisher<TestMsg> = Publisher::new(tx);
        let pub2 = pub1.clone();

        pub2.publish(TestMsg { value: 42 }).await.unwrap();

        let envelope = rx.recv().await.unwrap();
        assert_eq!(envelope.type_id, 1);
    }
}
