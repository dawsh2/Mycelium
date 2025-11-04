//! TLV Codec - Registry-Free Encoder/Decoder
//!
//! Implements bijective wire protocol for TLV messages:
//! ```text
//! ┌──────────┬──────────┬─────────────────┐
//! │ Type ID  │ Length   │ Payload         │
//! │ (u16 LE) │ (u32 LE) │ (zerocopy)      │
//! └──────────┴──────────┴─────────────────┘
//! ```
//!
//! ## Bijection Guarantee
//!
//! - **Encode**: Message → [TYPE_ID, LENGTH, PAYLOAD]
//! - **Decode**: [TYPE_ID, LENGTH, PAYLOAD] → Message
//! - **No Registry**: Direct TYPE_ID → Type mapping via match statement
//!
//! ## Performance
//!
//! - Zero-copy serialization with zerocopy
//! - Compile-time TYPE_ID constants (no HashMap lookup)
//! - Clean 6-byte header (no padding needed)
//! - Fixed 6-byte header overhead

use crate::Message;
use crate::{BatchOperation, CounterUpdate, TextMessage};
use zerocopy::{AsBytes, FromBytes};

/// Header size: 2 bytes (type_id) + 4 bytes (length) = 6 bytes
pub const HEADER_SIZE: usize = 6;

/// Maximum payload size (16MB - reasonable limit for in-memory messages)
pub const MAX_PAYLOAD_SIZE: usize = 16 * 1024 * 1024;

/// Policy for handling unknown message types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownTypePolicy {
    /// Skip unknown types (forward compatibility)
    Skip,

    /// Store unknown types as opaque blobs (for inspection/debugging)
    Store,

    /// Fail on unknown types (strict mode, default)
    Fail,
}

impl Default for UnknownTypePolicy {
    fn default() -> Self {
        UnknownTypePolicy::Fail
    }
}

/// Codec errors
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CodecError {
    #[error("Message too small: need {need} bytes, got {got}")]
    MessageTooSmall { need: usize, got: usize },

    #[error("Payload too large: {size} bytes (max {max})")]
    PayloadTooLarge { size: usize, max: usize },

    #[error("Unknown message type: {0}")]
    UnknownType(u16),

    #[error("Serialization failed: {0}")]
    SerializationFailed(String),

    #[error("Deserialization failed: {0}")]
    DeserializationFailed(String),

    #[error("Length mismatch: header says {expected}, buffer has {actual}")]
    LengthMismatch { expected: usize, actual: usize },
}

pub type CodecResult<T> = Result<T, CodecError>;

/// Encode a message to TLV wire format
///
/// ## Example
/// ```
/// use mycelium_protocol::{CounterUpdate, encode_message};
///
/// let msg = CounterUpdate { counter_id: 1, value: 100, delta: 5 };
/// let bytes = encode_message(&msg).unwrap();
///
/// // bytes = [TYPE_ID(u16), LENGTH(u32), PAYLOAD(zerocopy)]
/// ```
pub fn encode_message<M: Message + AsBytes>(message: &M) -> CodecResult<Vec<u8>> {
    // Serialize message with zerocopy
    let payload = message.as_bytes();

    // Check payload size
    if payload.len() > MAX_PAYLOAD_SIZE {
        return Err(CodecError::PayloadTooLarge {
            size: payload.len(),
            max: MAX_PAYLOAD_SIZE,
        });
    }

    // Build TLV: [type_id: u16][length: u32][payload: bytes]
    let mut tlv = Vec::with_capacity(HEADER_SIZE + payload.len());

    // Type ID (little-endian u16)
    tlv.extend_from_slice(&M::TYPE_ID.to_le_bytes());

    // Length (little-endian u32)
    tlv.extend_from_slice(&(payload.len() as u32).to_le_bytes());

    // Payload
    tlv.extend_from_slice(payload);

    Ok(tlv)
}

/// TLV header parsed from wire format
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TlvHeader {
    pub type_id: u16,
    pub length: u32,
}

/// Parse TLV header from bytes
///
/// Returns header and offset to payload start.
pub fn parse_header(bytes: &[u8]) -> CodecResult<(TlvHeader, usize)> {
    if bytes.len() < HEADER_SIZE {
        return Err(CodecError::MessageTooSmall {
            need: HEADER_SIZE,
            got: bytes.len(),
        });
    }

    let type_id = u16::from_le_bytes([bytes[0], bytes[1]]);
    let length = u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]);

    // Validate total message size
    let payload_end = HEADER_SIZE + length as usize;
    if bytes.len() < payload_end {
        return Err(CodecError::MessageTooSmall {
            need: payload_end,
            got: bytes.len(),
        });
    }

    // Validate payload size is reasonable
    if length as usize > MAX_PAYLOAD_SIZE {
        return Err(CodecError::PayloadTooLarge {
            size: length as usize,
            max: MAX_PAYLOAD_SIZE,
        });
    }

    Ok((TlvHeader { type_id, length }, HEADER_SIZE))
}

/// Decode TLV message to specific type (zero-copy)
///
/// ## Example
/// ```no_run
/// use mycelium_protocol::{CounterUpdate, decode_message};
///
/// let bytes = &[/* TLV bytes */];
/// let msg: CounterUpdate = decode_message(bytes).unwrap();
/// ```
pub fn decode_message<M: Message + FromBytes>(bytes: &[u8]) -> CodecResult<M> {
    let (header, payload_offset) = parse_header(bytes)?;

    // Verify type ID matches expected type
    if header.type_id != M::TYPE_ID {
        return Err(CodecError::UnknownType(header.type_id));
    }

    let payload = &bytes[payload_offset..payload_offset + header.length as usize];

    // Zero-copy deserialize with zerocopy
    M::read_from(payload).ok_or_else(|| {
        CodecError::DeserializationFailed(format!(
            "Failed to deserialize {}-byte payload",
            payload.len()
        ))
    })
}

/// Decode TLV to any supported message type (registry-free!)
///
/// Uses compile-time match on TYPE_ID - no HashMap, no runtime overhead.
///
/// ## Example
/// ```no_run
/// use mycelium_protocol::{decode_any_message, AnyMessage};
///
/// let bytes = &[/* TLV bytes */];
/// match decode_any_message(bytes).unwrap() {
///     AnyMessage::TextMessage(msg) => println!("Text message"),
///     AnyMessage::CounterUpdate(msg) => println!("Counter: {}", msg.value),
///     AnyMessage::BatchOperation(msg) => println!("Batch op"),
///     AnyMessage::Unknown(msg) => println!("Unknown type: {}", msg.type_id),
/// }
/// ```
pub fn decode_any_message(bytes: &[u8]) -> CodecResult<AnyMessage> {
    decode_any_message_with_policy(bytes, UnknownTypePolicy::default())
}

/// Decode TLV to any supported message type with custom unknown type handling
///
/// ## Example - Skip unknown types
/// ```no_run
/// use mycelium_protocol::{decode_any_message_with_policy, UnknownTypePolicy};
///
/// let bytes = &[/* TLV bytes */];
/// let result = decode_any_message_with_policy(bytes, UnknownTypePolicy::Skip);
/// // Returns Ok with Unknown variant for unknown types
/// ```
///
/// ## Example - Store unknown types
/// ```no_run
/// use mycelium_protocol::{decode_any_message_with_policy, UnknownTypePolicy, AnyMessage};
///
/// let bytes = &[/* TLV bytes */];
/// match decode_any_message_with_policy(bytes, UnknownTypePolicy::Store).unwrap() {
///     AnyMessage::Unknown(msg) => {
///         println!("Unknown type {}, payload: {:?}", msg.type_id, msg.payload);
///     }
///     _ => {}
/// }
/// ```
pub fn decode_any_message_with_policy(
    bytes: &[u8],
    policy: UnknownTypePolicy,
) -> CodecResult<AnyMessage> {
    let (header, payload_offset) = parse_header(bytes)?;
    let payload = &bytes[payload_offset..payload_offset + header.length as usize];

    // Direct match - no registry lookup!
    match header.type_id {
        TextMessage::TYPE_ID => {
            let msg = TextMessage::read_from(payload).ok_or_else(|| {
                CodecError::DeserializationFailed(format!(
                    "Failed to deserialize TextMessage from {}-byte payload",
                    payload.len()
                ))
            })?;
            Ok(AnyMessage::TextMessage(msg))
        }

        CounterUpdate::TYPE_ID => {
            let msg = CounterUpdate::read_from(payload).ok_or_else(|| {
                CodecError::DeserializationFailed(format!(
                    "Failed to deserialize CounterUpdate from {}-byte payload",
                    payload.len()
                ))
            })?;
            Ok(AnyMessage::CounterUpdate(msg))
        }

        BatchOperation::TYPE_ID => {
            let msg = BatchOperation::read_from(payload).ok_or_else(|| {
                CodecError::DeserializationFailed(format!(
                    "Failed to deserialize BatchOperation from {}-byte payload",
                    payload.len()
                ))
            })?;
            Ok(AnyMessage::BatchOperation(msg))
        }

        unknown => {
            // Handle unknown type based on policy
            match policy {
                UnknownTypePolicy::Fail => Err(CodecError::UnknownType(unknown)),

                UnknownTypePolicy::Skip => {
                    // For skip, we could return an Option, but to keep the signature simple,
                    // we'll return Unknown variant and let caller decide
                    Ok(AnyMessage::Unknown(UnknownMessage {
                        type_id: unknown,
                        payload: payload.to_vec(),
                    }))
                }

                UnknownTypePolicy::Store => Ok(AnyMessage::Unknown(UnknownMessage {
                    type_id: unknown,
                    payload: payload.to_vec(),
                })),
            }
        }
    }
}

/// Opaque unknown message (stored when using UnknownTypePolicy::Store)
#[derive(Debug, Clone, PartialEq)]
pub struct UnknownMessage {
    pub type_id: u16,
    pub payload: Vec<u8>,
}

/// Enum of all supported message types
///
/// Used by `decode_any_message()` for type-safe dynamic dispatch.
#[derive(Debug, Clone, PartialEq)]
pub enum AnyMessage {
    TextMessage(TextMessage),
    CounterUpdate(CounterUpdate),
    BatchOperation(BatchOperation),
    Unknown(UnknownMessage),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_text_message() {
        use crate::fixed_vec::FixedStr;

        // Create a TextMessage manually since we removed constructors
        let original = TextMessage {
            sender: FixedStr::from_str("alice").unwrap(),
            content: FixedStr::from_str("Hello, World!").unwrap(),
            timestamp: 1234567890,
        };

        // Encode
        let bytes = encode_message(&original).unwrap();

        // Verify header
        assert_eq!(&bytes[0..2], &TextMessage::TYPE_ID.to_le_bytes());
        assert!(bytes.len() > HEADER_SIZE);

        // Decode
        let decoded: TextMessage = decode_message(&bytes).unwrap();

        assert_eq!(decoded.sender.as_str().unwrap(), "alice");
        assert_eq!(decoded.content.as_str().unwrap(), "Hello, World!");
        assert_eq!(decoded.timestamp, 1234567890);
    }

    #[test]
    fn test_encode_decode_counter_update() {
        let original = CounterUpdate {
            counter_id: 42,
            value: 100,
            delta: 5,
        };

        let bytes = encode_message(&original).unwrap();
        let decoded: CounterUpdate = decode_message(&bytes).unwrap();

        assert_eq!(decoded.counter_id, 42);
        assert_eq!(decoded.value, 100);
        assert_eq!(decoded.delta, 5);
    }

    #[test]
    fn test_encode_decode_batch_operation() {
        use crate::fixed_vec::FixedVec;

        let items = vec![[1; 20], [2; 20], [3; 20]];
        let original = BatchOperation {
            operation_id: 999,
            items: FixedVec::from_slice(&items).unwrap(),
            total_count: 3,
        };

        let bytes = encode_message(&original).unwrap();
        let decoded: BatchOperation = decode_message(&bytes).unwrap();

        assert_eq!(decoded.operation_id, 999);
        assert_eq!(decoded.items.len(), 3);
        assert_eq!(decoded.total_count, 3);
    }

    #[test]
    fn test_decode_any_message() {
        use crate::fixed_vec::FixedStr;

        // Test TextMessage
        let msg = TextMessage {
            sender: FixedStr::from_str("bob").unwrap(),
            content: FixedStr::from_str("test").unwrap(),
            timestamp: 999,
        };
        let bytes = encode_message(&msg).unwrap();

        match decode_any_message(&bytes).unwrap() {
            AnyMessage::TextMessage(decoded) => {
                assert_eq!(decoded.sender.as_str().unwrap(), "bob");
            }
            _ => panic!("Expected TextMessage"),
        }

        // Test CounterUpdate
        let counter = CounterUpdate {
            counter_id: 5,
            value: 10,
            delta: 1,
        };
        let bytes = encode_message(&counter).unwrap();

        match decode_any_message(&bytes).unwrap() {
            AnyMessage::CounterUpdate(msg) => assert_eq!(msg.counter_id, 5),
            _ => panic!("Expected CounterUpdate"),
        }
    }

    #[test]
    fn test_bijection_property() {
        // Encode → Decode → Encode should produce same bytes
        let original = CounterUpdate {
            counter_id: 1,
            value: 50,
            delta: 10,
        };

        let bytes1 = encode_message(&original).unwrap();
        let decoded: CounterUpdate = decode_message(&bytes1).unwrap();
        let bytes2 = encode_message(&decoded).unwrap();

        assert_eq!(bytes1, bytes2); // Perfect bijection!
    }

    #[test]
    fn test_parse_header() {
        use crate::fixed_vec::FixedStr;

        let msg = TextMessage {
            sender: FixedStr::from_str("test").unwrap(),
            content: FixedStr::from_str("header test").unwrap(),
            timestamp: 0,
        };
        let bytes = encode_message(&msg).unwrap();

        let (header, offset) = parse_header(&bytes).unwrap();

        assert_eq!(header.type_id, TextMessage::TYPE_ID);
        assert_eq!(offset, HEADER_SIZE);
        assert!(header.length > 0);
    }

    #[test]
    fn test_error_message_too_small() {
        let bytes = vec![0, 1, 2]; // Only 3 bytes, need 6

        let result = parse_header(&bytes);
        assert!(matches!(result, Err(CodecError::MessageTooSmall { .. })));
    }

    #[test]
    fn test_error_unknown_type() {
        // Create valid TLV with unknown type ID
        let mut bytes = vec![0; HEADER_SIZE + 10];
        bytes[0..2].copy_from_slice(&9999u16.to_le_bytes()); // Unknown type
        bytes[2..6].copy_from_slice(&10u32.to_le_bytes()); // Length

        let result = decode_any_message(&bytes);
        assert!(matches!(result, Err(CodecError::UnknownType(9999))));
    }

    #[test]
    fn test_roundtrip_all_types() {
        use crate::fixed_vec::{FixedStr, FixedVec};

        // TextMessage
        let msg = TextMessage {
            sender: FixedStr::from_str("alice").unwrap(),
            content: FixedStr::from_str("test").unwrap(),
            timestamp: 123,
        };
        let bytes = encode_message(&msg).unwrap();
        let decoded: TextMessage = decode_message(&bytes).unwrap();
        assert_eq!(
            msg.sender.as_str().unwrap(),
            decoded.sender.as_str().unwrap()
        );

        // CounterUpdate
        let counter = CounterUpdate {
            counter_id: 42,
            value: 100,
            delta: 5,
        };
        let bytes = encode_message(&counter).unwrap();
        let decoded: CounterUpdate = decode_message(&bytes).unwrap();
        assert_eq!(counter.counter_id, decoded.counter_id);

        // BatchOperation
        let items = vec![[1; 20], [2; 20]];
        let batch = BatchOperation {
            operation_id: 999,
            items: FixedVec::from_slice(&items).unwrap(),
            total_count: 2,
        };
        let bytes = encode_message(&batch).unwrap();
        let decoded: BatchOperation = decode_message(&bytes).unwrap();
        assert_eq!(batch.operation_id, decoded.operation_id);
    }

    #[test]
    fn test_unknown_type_fail_policy() {
        // Create a fake message with unknown type_id
        let unknown_type_id: u16 = 9999; // Not in our schema
        let payload = vec![1, 2, 3, 4];

        // Build TLV manually
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&unknown_type_id.to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&payload);

        // Default policy (Fail) should return error
        let result = decode_any_message(&bytes);
        assert!(matches!(result, Err(CodecError::UnknownType(9999))));
    }

    #[test]
    fn test_unknown_type_skip_policy() {
        // Create a fake message with unknown type_id
        let unknown_type_id: u16 = 9999;
        let payload = vec![1, 2, 3, 4];

        // Build TLV manually
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&unknown_type_id.to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&payload);

        // Skip policy should return Unknown variant
        let result = decode_any_message_with_policy(&bytes, UnknownTypePolicy::Skip);
        assert!(result.is_ok());

        match result.unwrap() {
            AnyMessage::Unknown(msg) => {
                assert_eq!(msg.type_id, 9999);
                assert_eq!(msg.payload, vec![1, 2, 3, 4]);
            }
            _ => panic!("Expected Unknown variant"),
        }
    }

    #[test]
    fn test_unknown_type_store_policy() {
        // Create a fake message with unknown type_id
        let unknown_type_id: u16 = 8888;
        let payload = vec![0xDE, 0xAD, 0xBE, 0xEF];

        // Build TLV manually
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&unknown_type_id.to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&payload);

        // Store policy should return Unknown variant with payload
        let result = decode_any_message_with_policy(&bytes, UnknownTypePolicy::Store);
        assert!(result.is_ok());

        match result.unwrap() {
            AnyMessage::Unknown(msg) => {
                assert_eq!(msg.type_id, 8888);
                assert_eq!(msg.payload, vec![0xDE, 0xAD, 0xBE, 0xEF]);
            }
            _ => panic!("Expected Unknown variant"),
        }
    }

    #[test]
    fn test_known_type_with_policy() {
        use crate::fixed_vec::FixedStr;

        // Known type should decode normally regardless of policy
        let msg = TextMessage {
            sender: FixedStr::from_str("test").unwrap(),
            content: FixedStr::from_str("content").unwrap(),
            timestamp: 999,
        };
        let bytes = encode_message(&msg).unwrap();

        // Should work with all policies
        for policy in [
            UnknownTypePolicy::Fail,
            UnknownTypePolicy::Skip,
            UnknownTypePolicy::Store,
        ] {
            let result = decode_any_message_with_policy(&bytes, policy);
            assert!(result.is_ok());

            match result.unwrap() {
                AnyMessage::TextMessage(decoded) => {
                    assert_eq!(decoded.sender.as_str().unwrap(), "test");
                }
                _ => panic!("Expected TextMessage"),
            }
        }
    }
}
