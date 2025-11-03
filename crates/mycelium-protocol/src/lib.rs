pub mod codec;
pub mod envelope;
pub mod fixed_vec;
pub mod message;
pub mod routing;
// pub mod messages; // Manual implementation (for comparison) - disabled during zerocopy migration

// Generated message types from contracts.yaml
pub mod generated;

pub use codec::{
    decode_any_message, decode_message, encode_message, parse_header, AnyMessage, CodecError,
    CodecResult, TlvHeader, HEADER_SIZE, MAX_PAYLOAD_SIZE,
};
pub use envelope::{Envelope, EnvelopeError};
pub use fixed_vec::{FixedStr, FixedVec, FixedVecError};
pub use message::Message;
pub use routing::{ActorId, CorrelationId, Destination};

// Export generated types as the primary API
pub use generated::{ArbitrageSignal, InstrumentMeta, PoolStateUpdate, ValidationError, U256};
