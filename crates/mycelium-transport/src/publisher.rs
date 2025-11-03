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
    /// Messages are delivered to all active subscribers.
    /// If there are no subscribers, the message is silently dropped.
    pub async fn publish(&self, msg: M) -> Result<()> {
        let envelope = Envelope::new(msg);
        let _ = self.tx.send(envelope);
        Ok(())
    }

    /// Try to publish a message (non-blocking)
    ///
    /// Returns immediately. Messages are silently dropped if no subscribers.
    pub fn try_publish(&self, msg: M) -> Result<()> {
        let envelope = Envelope::new(msg);
        let _ = self.tx.send(envelope);
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
    use rkyv::{Archive, Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Archive, Serialize, Deserialize)]
    #[archive(check_bytes)]
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

        // Broadcast allows sending with no subscribers
        // Check that we can detect this via subscriber_count
        assert_eq!(pub_.subscriber_count(), 0);

        // Publishing succeeds even with no subscribers
        let result = pub_.publish(TestMsg { value: 42 }).await;
        assert!(result.is_ok());
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
