use crate::message::Message;
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
pub struct Envelope {
    /// Message type ID
    pub type_id: u16,

    /// Topic this message belongs to
    pub topic: String,

    /// Type-erased payload (Arc for zero-copy sharing)
    payload: Arc<dyn Any + Send + Sync>,
}

impl std::fmt::Debug for Envelope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Envelope")
            .field("type_id", &self.type_id)
            .field("topic", &self.topic)
            .field("payload", &"<opaque>")
            .finish()
    }
}

impl Envelope {
    /// Create a new envelope from a message
    pub fn new<M: Message>(msg: M) -> Self {
        Self {
            type_id: M::TYPE_ID,
            topic: M::TOPIC.to_string(),
            payload: Arc::new(msg) as Arc<dyn Any + Send + Sync>,
        }
    }

    /// Create an envelope from raw components (for transport layer use)
    pub fn from_raw(type_id: u16, topic: String, payload: Arc<dyn Any + Send + Sync>) -> Self {
        Self {
            type_id,
            topic,
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
            payload: Arc::clone(&self.payload),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::impl_message;
    use rkyv::{Archive, Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Archive, Serialize, Deserialize)]
    #[archive(check_bytes)]
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
        #[derive(Debug, Clone, Archive, Serialize, Deserialize)]
        #[archive(check_bytes)]
        struct OtherMessage {
            data: u64,
        }

        impl_message!(OtherMessage, 100, "other");

        let msg = TestMessage { value: 42 };
        let envelope = Envelope::new(msg);

        let result = envelope.downcast::<OtherMessage>();
        assert!(matches!(
            result,
            Err(EnvelopeError::TypeMismatch { expected: 100, got: 99 })
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
