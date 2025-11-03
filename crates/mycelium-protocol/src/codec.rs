//! TLV Codec - Registry-Free Encoder/Decoder
//!
//! Implements bijective wire protocol for TLV messages:
//! ```
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
use crate::{ArbitrageSignal, InstrumentMeta, PoolStateUpdate};
use zerocopy::{AsBytes, FromBytes};

/// Header size: 2 bytes (type_id) + 4 bytes (length) = 6 bytes
pub const HEADER_SIZE: usize = 6;

/// Maximum payload size (16MB - reasonable limit for in-memory messages)
pub const MAX_PAYLOAD_SIZE: usize = 16 * 1024 * 1024;

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
/// use mycelium_protocol::{InstrumentMeta, encode_message};
///
/// let msg = InstrumentMeta::new([1; 20], "WETH", 18, 137).unwrap();
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

    Ok((
        TlvHeader {
            type_id,
            length,
        },
        HEADER_SIZE,
    ))
}

/// Decode TLV message to specific type (zero-copy)
///
/// ## Example
/// ```
/// use mycelium_protocol::{InstrumentMeta, decode_message};
///
/// let bytes = &[/* TLV bytes */];
/// let msg: InstrumentMeta = decode_message(bytes).unwrap();
/// ```
pub fn decode_message<M: Message + FromBytes>(bytes: &[u8]) -> CodecResult<M> {
    let (header, payload_offset) = parse_header(bytes)?;

    // Verify type ID matches expected type
    if header.type_id != M::TYPE_ID {
        return Err(CodecError::UnknownType(header.type_id));
    }

    let payload = &bytes[payload_offset..payload_offset + header.length as usize];

    // Zero-copy deserialize with zerocopy
    M::read_from(payload)
        .ok_or_else(|| CodecError::DeserializationFailed(
            format!("Failed to deserialize {}-byte payload", payload.len())
        ))
}

/// Decode TLV to any supported message type (registry-free!)
///
/// Uses compile-time match on TYPE_ID - no HashMap, no runtime overhead.
///
/// ## Example
/// ```
/// use mycelium_protocol::decode_any_message;
///
/// let bytes = &[/* TLV bytes */];
/// match decode_any_message(bytes).unwrap() {
///     AnyMessage::InstrumentMeta(msg) => println!("Token: {}", msg.symbol_str()),
///     AnyMessage::PoolStateUpdate(msg) => println!("Pool state"),
///     AnyMessage::ArbitrageSignal(msg) => println!("Arb signal"),
/// }
/// ```
pub fn decode_any_message(bytes: &[u8]) -> CodecResult<AnyMessage> {
    let (header, payload_offset) = parse_header(bytes)?;
    let payload = &bytes[payload_offset..payload_offset + header.length as usize];

    // Direct match - no registry lookup!
    match header.type_id {
        InstrumentMeta::TYPE_ID => {
            let msg = InstrumentMeta::read_from(payload)
                .ok_or_else(|| CodecError::DeserializationFailed(
                    format!("Failed to deserialize InstrumentMeta from {}-byte payload", payload.len())
                ))?;
            Ok(AnyMessage::InstrumentMeta(msg))
        }

        PoolStateUpdate::TYPE_ID => {
            let msg = PoolStateUpdate::read_from(payload)
                .ok_or_else(|| CodecError::DeserializationFailed(
                    format!("Failed to deserialize PoolStateUpdate from {}-byte payload", payload.len())
                ))?;
            Ok(AnyMessage::PoolStateUpdate(msg))
        }

        ArbitrageSignal::TYPE_ID => {
            let msg = ArbitrageSignal::read_from(payload)
                .ok_or_else(|| CodecError::DeserializationFailed(
                    format!("Failed to deserialize ArbitrageSignal from {}-byte payload", payload.len())
                ))?;
            Ok(AnyMessage::ArbitrageSignal(msg))
        }

        unknown => Err(CodecError::UnknownType(unknown)),
    }
}

/// Enum of all supported message types
///
/// Used by `decode_any_message()` for type-safe dynamic dispatch.
#[derive(Debug, Clone, PartialEq)]
pub enum AnyMessage {
    InstrumentMeta(InstrumentMeta),
    PoolStateUpdate(PoolStateUpdate),
    ArbitrageSignal(ArbitrageSignal),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::U256;

    #[test]
    fn test_encode_decode_instrument_meta() {
        let original = InstrumentMeta::new([1; 20], "WETH", 18, 137).unwrap();

        // Encode
        let bytes = encode_message(&original).unwrap();

        // Verify header
        assert_eq!(&bytes[0..2], &InstrumentMeta::TYPE_ID.to_le_bytes());
        assert!(bytes.len() > HEADER_SIZE);

        // Decode
        let decoded: InstrumentMeta = decode_message(&bytes).unwrap();

        assert_eq!(decoded.symbol_str(), "WETH");
        assert_eq!(decoded.decimals, 18);
        assert_eq!(decoded.chain_id, 137);
    }

    #[test]
    fn test_encode_decode_pool_state() {
        let original = PoolStateUpdate::new_v2(
            [5; 20],
            1,
            U256::from(1000000),
            U256::from(2000000),
            54321,
        )
        .unwrap();

        let bytes = encode_message(&original).unwrap();
        let decoded: PoolStateUpdate = decode_message(&bytes).unwrap();

        assert_eq!(decoded.reserve0(), U256::from(1000000));
        assert_eq!(decoded.reserve1(), U256::from(2000000));
        assert_eq!(decoded.block_number, 54321);
    }

    #[test]
    fn test_encode_decode_arbitrage_signal() {
        let path = [[1; 20], [2; 20], [3; 20]];
        let original =
            ArbitrageSignal::new(999, &path, 250.75, U256::from(42000), 99999).unwrap();

        let bytes = encode_message(&original).unwrap();
        let decoded: ArbitrageSignal = decode_message(&bytes).unwrap();

        assert_eq!(decoded.opportunity_id, 999);
        assert_eq!(decoded.hop_count(), 3);
        assert_eq!(decoded.estimated_profit_usd, 250.75);
    }

    #[test]
    fn test_decode_any_message() {
        // Test InstrumentMeta
        let meta = InstrumentMeta::new([1; 20], "USDC", 6, 137).unwrap();
        let bytes = encode_message(&meta).unwrap();

        match decode_any_message(&bytes).unwrap() {
            AnyMessage::InstrumentMeta(msg) => assert_eq!(msg.symbol_str(), "USDC"),
            _ => panic!("Expected InstrumentMeta"),
        }

        // Test PoolStateUpdate
        let pool = PoolStateUpdate::new_v2([2; 20], 1, U256::from(100), U256::from(200), 123)
            .unwrap();
        let bytes = encode_message(&pool).unwrap();

        match decode_any_message(&bytes).unwrap() {
            AnyMessage::PoolStateUpdate(msg) => assert_eq!(msg.block_number, 123),
            _ => panic!("Expected PoolStateUpdate"),
        }
    }

    #[test]
    fn test_bijection_property() {
        // Encode → Decode → Encode should produce same bytes
        let original = InstrumentMeta::new([1; 20], "WETH", 18, 137).unwrap();

        let bytes1 = encode_message(&original).unwrap();
        let decoded: InstrumentMeta = decode_message(&bytes1).unwrap();
        let bytes2 = encode_message(&decoded).unwrap();

        assert_eq!(bytes1, bytes2); // Perfect bijection!
    }

    #[test]
    fn test_parse_header() {
        let msg = InstrumentMeta::new([1; 20], "WETH", 18, 137).unwrap();
        let bytes = encode_message(&msg).unwrap();

        let (header, offset) = parse_header(&bytes).unwrap();

        assert_eq!(header.type_id, InstrumentMeta::TYPE_ID);
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
        // InstrumentMeta
        let meta = InstrumentMeta::new([1; 20], "DAI", 18, 137).unwrap();
        let bytes = encode_message(&meta).unwrap();
        let decoded: InstrumentMeta = decode_message(&bytes).unwrap();
        assert_eq!(meta.symbol_str(), decoded.symbol_str());

        // PoolStateUpdate V2
        let pool = PoolStateUpdate::new_v2([2; 20], 1, U256::from(1000), U256::from(2000), 100)
            .unwrap();
        let bytes = encode_message(&pool).unwrap();
        let decoded: PoolStateUpdate = decode_message(&bytes).unwrap();
        assert_eq!(pool.reserve0(), decoded.reserve0());

        // PoolStateUpdate V3
        let pool = PoolStateUpdate::new_v3(
            [3; 20],
            2,
            U256::from(5000),
            U256::from(123456),
            -100,
            200,
        )
        .unwrap();
        let bytes = encode_message(&pool).unwrap();
        let decoded: PoolStateUpdate = decode_message(&bytes).unwrap();
        assert_eq!(pool.liquidity(), decoded.liquidity());

        // ArbitrageSignal
        let path = [[1; 20], [2; 20]];
        let signal = ArbitrageSignal::new(123, &path, 50.0, U256::from(1000), 500).unwrap();
        let bytes = encode_message(&signal).unwrap();
        let decoded: ArbitrageSignal = decode_message(&bytes).unwrap();
        assert_eq!(signal.opportunity_id, decoded.opportunity_id);
    }
}
