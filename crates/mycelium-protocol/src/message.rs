use std::fmt::Debug;
use zerocopy::{AsBytes, FromBytes};

/// Core trait for all messages in the Mycelium system.
///
/// Messages must be:
/// - Zero-copy serializable (zerocopy)
/// - Send + Sync (can be shared across threads)
/// - Have a unique type ID and topic
pub trait Message: AsBytes + FromBytes + Send + Sync + Debug + Copy + 'static {
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
    use zerocopy::{AsBytes, FromBytes, FromZeroes};

    #[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
    #[repr(C)]
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
    fn test_zerocopy_roundtrip() {
        let original = TestMessage {
            id: 42,
            value: 100,
        };

        // Serialize with zerocopy
        let bytes = original.as_bytes();

        // Deserialize (zero-copy)
        let deserialized = TestMessage::read_from(bytes).unwrap();

        assert_eq!(original, deserialized);
    }
}
