use crate::bridge::{BridgeFanout, BridgeFrame};
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
    bridge_fanout: BridgeFanout,
    _phantom: PhantomData<M>,
}

impl<M: Message> Publisher<M> {
    pub(crate) fn with_bridge(
        tx: broadcast::Sender<Envelope>,
        bridge_fanout: BridgeFanout,
    ) -> Self {
        Self {
            tx,
            bridge_fanout,
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
        self.emit_bridge_frame(&msg);

        let envelope = Envelope::new(msg);
        let receiver_count = self.send_envelope(envelope)?;

        if receiver_count == 0 && !self.bridge_fanout.has_subscribers() {
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
        self.emit_bridge_frame(&msg);

        let envelope = Envelope::new(msg);
        let receiver_count = self.send_envelope(envelope)?;

        if receiver_count == 0 && !self.bridge_fanout.has_subscribers() {
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
        self.emit_bridge_frame(&msg);

        let envelope = Envelope::new(msg);
        let receiver_count = self.send_envelope(envelope)?;

        if receiver_count == 0 && !self.bridge_fanout.has_subscribers() {
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

    fn emit_bridge_frame(&self, msg: &M) {
        if !self.bridge_fanout.has_subscribers() {
            return;
        }

        let frame = BridgeFrame::from_message(msg);
        self.bridge_fanout.send(frame);
    }

    fn send_envelope(&self, envelope: Envelope) -> Result<usize> {
        // If there are no local subscribers, skip the broadcast send entirely.
        // Higher-level logic will surface a NoSubscribers error when appropriate
        // (based on both local and bridge clients).
        if self.tx.receiver_count() == 0 {
            return Ok(0);
        }

        self.tx
            .send(envelope)
            .map_err(|_| crate::TransportError::SendFailed)
    }
}

impl<M: Message> Clone for Publisher<M> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            bridge_fanout: self.bridge_fanout.clone(),
            _phantom: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mycelium_protocol::{codec::HEADER_SIZE, impl_message};
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
        let pub_: Publisher<TestMsg> =
            Publisher::with_bridge(tx.clone(), crate::bridge::BridgeFanout::new(1));

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
        let pub_: Publisher<TestMsg> =
            Publisher::with_bridge(tx, crate::bridge::BridgeFanout::new(1));

        pub_.publish(TestMsg { value: 42 }).await.unwrap();

        let envelope = rx.recv().await.unwrap();
        assert_eq!(envelope.type_id, 1);
    }

    #[tokio::test]
    async fn test_publisher_clone() {
        let (tx, mut rx) = broadcast::channel(10);
        let fanout = crate::bridge::BridgeFanout::new(1);
        let pub1: Publisher<TestMsg> = Publisher::with_bridge(tx, fanout.clone());
        let pub2 = pub1.clone();

        pub2.publish(TestMsg { value: 42 }).await.unwrap();

        let envelope = rx.recv().await.unwrap();
        assert_eq!(envelope.type_id, 1);
    }

    #[tokio::test]
    async fn test_bridge_fanout_emits_frames() {
        let (tx, mut rx) = broadcast::channel(10);
        let fanout = crate::bridge::BridgeFanout::new(2);
        let pub_: Publisher<TestMsg> = Publisher::with_bridge(tx, fanout.clone());

        let mut bridge_rx = fanout.subscribe();

        // Keep a local subscriber alive so publish() succeeds
        tokio::spawn(async move {
            let _ = rx.recv().await;
        });

        pub_.publish(TestMsg { value: 7 }).await.unwrap();

        let frame = bridge_rx.recv().await.unwrap();
        assert_eq!(frame.type_id, TestMsg::TYPE_ID);
        assert_eq!(frame.topic.as_ref(), TestMsg::TOPIC);
        assert_eq!(
            frame.bytes.len(),
            HEADER_SIZE + std::mem::size_of::<TestMsg>()
        );
    }
}
