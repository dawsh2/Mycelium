pub mod codec;
pub mod codegen; // Public code generation API
pub mod envelope;
pub mod fixed_vec;
pub mod message;
pub mod routing;
pub mod schema_compat;
// pub mod messages; // Manual implementation (for comparison) - disabled during zerocopy migration

// Generated message types from contracts.yaml
pub mod generated;

pub use codec::{
    decode_any_message, decode_any_message_with_policy, decode_message, encode_message,
    parse_header, AnyMessage, CodecError, CodecResult, TlvHeader, UnknownMessage,
    UnknownTypePolicy, HEADER_SIZE, MAX_PAYLOAD_SIZE,
};
pub use envelope::{Envelope, EnvelopeError};
pub use fixed_vec::{FixedStr, FixedVec, FixedVecError};
pub use message::Message;

// Note: impl_zerocopy_for_fixed_vec! and impl_zerocopy_for_fixed_str! macros
// are available at the crate root due to #[macro_export]
pub use routing::{ActorId, CorrelationId, Destination, TraceId};
pub use schema_compat::{
    check_compatibility, CompatibilityIssue, CompatibilityLevel, CompatibilityReport, FieldSchema,
    MessageSchema, Schema,
};

// Export generated types as the primary API
pub use generated::{
    // Example message types from contracts.yaml
    TextMessage,
    CounterUpdate,
    BatchOperation,
    ValidationError,
};
