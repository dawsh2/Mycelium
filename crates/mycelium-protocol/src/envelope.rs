use crate::message::Message;
use crate::routing::{CorrelationId, Destination};
use std::any::Any;
use std::sync::Arc;
use thiserror::Error;

/// Error types for envelope operations
#[derive(Error, Debug)]
pub enum EnvelopeError {
    #[error("Type mismatch: expected type {expected}, got type {got}")]
    TypeMismatch { expected: u16, got: u16 },

    #[error("Downcast failed: message is not of the expected type")]
    DowncastFailed,
}

/// Message envelope containing type information and payload
///
/// Envelopes allow type-erased message passing while preserving
/// type safety through downcasting.
///
/// ## Phase 1: Actor-Ready Foundation
///
/// Optional metadata fields support future actor system features:
/// - `sequence`: Message ordering (for OrderedSubscriber and actor mailboxes)
/// - `destination`: Routing hint (Broadcast, Unicast to actor, Partition)
/// - `correlation_id`: Request/reply correlation
///
/// These fields are `Option<T>` for zero cost when unused.
pub struct Envelope {
    /// Message type ID
    pub type_id: u16,

    /// Topic this message belongs to
    pub topic: String,

    /// Optional sequence number for ordered delivery
    ///
    /// When `Some(n)`, subscribers can use this to ensure FIFO ordering.
    /// When `None`, messages are delivered as-received (default).
    pub sequence: Option<u64>,

    /// Optional routing hint (Phase 1: Actor-ready)
    ///
    /// - `None` or `Some(Destination::Broadcast)`: Standard pub/sub (all subscribers)
    /// - `Some(Destination::Unicast(actor_id))`: Actor mailbox routing
    /// - `Some(Destination::Partition(hash))`: Partition-based routing
    pub destination: Option<Destination>,

    /// Optional correlation ID for request/reply (Phase 1: Actor-ready)
    ///
    /// Used to match responses to requests in async request/reply patterns.
    /// When `None`, this is a one-way message.
    pub correlation_id: Option<CorrelationId>,

    /// Type-erased payload (Arc for zero-copy sharing)
    payload: Arc<dyn Any + Send + Sync>,
}

impl std::fmt::Debug for Envelope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Envelope")
            .field("type_id", &self.type_id)
            .field("topic", &self.topic)
            .field("sequence", &self.sequence)
            .field("payload", &"<opaque>")
            .finish()
    }
}

impl Envelope {
    /// Create a new envelope from a message (no sequence number)
    pub fn new<M: Message>(msg: M) -> Self {
        Self {
            type_id: M::TYPE_ID,
            topic: M::TOPIC.to_string(),
            sequence: None,
            destination: None,
            correlation_id: None,
            payload: Arc::new(msg) as Arc<dyn Any + Send + Sync>,
        }
    }

    /// Create a new envelope with a sequence number (for ordered delivery)
    pub fn with_sequence<M: Message>(msg: M, sequence: u64) -> Self {
        Self {
            type_id: M::TYPE_ID,
            topic: M::TOPIC.to_string(),
            sequence: Some(sequence),
            destination: None,
            correlation_id: None,
            payload: Arc::new(msg) as Arc<dyn Any + Send + Sync>,
        }
    }

    /// Create an envelope from raw components (for internal transport layer use)
    ///
    /// **Note**: This is a low-level API used by transport implementations.
    /// Most users should use `Envelope::new()` instead.
    #[doc(hidden)]
    pub fn from_raw(type_id: u16, topic: String, payload: Arc<dyn Any + Send + Sync>) -> Self {
        Self {
            type_id,
            topic,
            sequence: None,
            destination: None,
            correlation_id: None,
            payload,
        }
    }

    /// Create an envelope from raw components with sequence number
    #[doc(hidden)]
    pub fn from_raw_with_sequence(
        type_id: u16,
        topic: String,
        sequence: Option<u64>,
        payload: Arc<dyn Any + Send + Sync>,
    ) -> Self {
        Self {
            type_id,
            topic,
            sequence,
            destination: None,
            correlation_id: None,
            payload,
        }
    }

    /// Attempt to downcast the envelope to a concrete message type
    pub fn downcast<M: Message>(self) -> Result<Arc<M>, EnvelopeError> {
        // Check type ID first
        if self.type_id != M::TYPE_ID {
            return Err(EnvelopeError::TypeMismatch {
                expected: M::TYPE_ID,
                got: self.type_id,
            });
        }

        // Attempt downcast
        self.payload
            .downcast::<M>()
            .map_err(|_| EnvelopeError::DowncastFailed)
    }

    /// Check if this envelope matches a specific message type
    pub fn is<M: Message>(&self) -> bool {
        self.type_id == M::TYPE_ID
    }

    /// Downcast the envelope payload to any type (for internal transport use)
    pub fn downcast_any<T: Any + Send + Sync>(self) -> Result<Arc<T>, EnvelopeError> {
        self.payload
            .downcast::<T>()
            .map_err(|_| EnvelopeError::DowncastFailed)
    }
}

impl Clone for Envelope {
    fn clone(&self) -> Self {
        Self {
            type_id: self.type_id,
            topic: self.topic.clone(),
            sequence: self.sequence,
            destination: self.destination,
            correlation_id: self.correlation_id,
            payload: Arc::clone(&self.payload),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::impl_message;
    use zerocopy::{AsBytes, FromBytes, FromZeroes};

    #[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
    #[repr(C)]
    struct TestMessage {
        value: u64,
    }

    impl_message!(TestMessage, 99, "test");

    #[test]
    fn test_envelope_creation() {
        let msg = TestMessage { value: 42 };
        let envelope = Envelope::new(msg);

        assert_eq!(envelope.type_id, 99);
        assert_eq!(envelope.topic, "test");
        assert!(envelope.is::<TestMessage>());
    }

    #[test]
    fn test_envelope_downcast_success() {
        let msg = TestMessage { value: 42 };
        let envelope = Envelope::new(msg.clone());

        let downcasted = envelope.downcast::<TestMessage>().unwrap();
        assert_eq!(downcasted.value, 42);
    }

    #[test]
    fn test_envelope_downcast_type_mismatch() {
        #[derive(Debug, Clone, Copy, AsBytes, FromBytes, FromZeroes)]
        #[repr(C)]
        struct OtherMessage {
            data: u64,
        }

        impl_message!(OtherMessage, 100, "other");

        let msg = TestMessage { value: 42 };
        let envelope = Envelope::new(msg);

        let result = envelope.downcast::<OtherMessage>();
        assert!(matches!(
            result,
            Err(EnvelopeError::TypeMismatch {
                expected: 100,
                got: 99
            })
        ));
    }

    #[test]
    fn test_envelope_clone() {
        let msg = TestMessage { value: 42 };
        let envelope = Envelope::new(msg);

        let cloned = envelope.clone();
        assert_eq!(cloned.type_id, 99);
        assert_eq!(cloned.topic, "test");

        // Both should be able to downcast
        let _ = cloned.downcast::<TestMessage>().unwrap();
    }
}
