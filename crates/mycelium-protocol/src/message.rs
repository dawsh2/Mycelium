use rkyv::{
    ser::serializers::AllocSerializer, Archive, Serialize,
};
use std::fmt::Debug;

/// Core trait for all messages in the Mycelium system.
///
/// Messages must be:
/// - Zero-copy serializable (rkyv)
/// - Send + Sync (can be shared across threads)
/// - Have a unique type ID and topic
pub trait Message: Archive + Serialize<AllocSerializer<256>> + Send + Sync + Debug + 'static {
    /// Unique message type ID (for TLV encoding)
    const TYPE_ID: u16;

    /// Topic this message belongs to (for pub/sub routing)
    const TOPIC: &'static str;
}

/// Helper macro to implement Message trait
#[macro_export]
macro_rules! impl_message {
    ($type:ty, $id:expr, $topic:expr) => {
        impl $crate::message::Message for $type {
            const TYPE_ID: u16 = $id;
            const TOPIC: &'static str = $topic;
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use rkyv::Deserialize;

    #[derive(Debug, Clone, PartialEq, Archive, Serialize, Deserialize)]
    #[archive(check_bytes)]
    struct TestMessage {
        id: u64,
        value: u64,
    }

    impl_message!(TestMessage, 1, "test");

    #[test]
    fn test_message_trait() {
        assert_eq!(TestMessage::TYPE_ID, 1);
        assert_eq!(TestMessage::TOPIC, "test");
    }

    #[test]
    fn test_rkyv_roundtrip() {
        let original = TestMessage {
            id: 42,
            value: 100,
        };

        // Serialize
        let bytes = rkyv::to_bytes::<_, 256>(&original).unwrap();

        // Deserialize (zero-copy)
        let archived = unsafe { rkyv::archived_root::<TestMessage>(&bytes) };
        let deserialized: TestMessage = archived.deserialize(&mut rkyv::Infallible).unwrap();

        assert_eq!(original, deserialized);
    }
}
