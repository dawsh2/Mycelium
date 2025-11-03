pub mod envelope;
pub mod fixed_vec;
pub mod message;
pub mod messages; // Manual implementation (for comparison)

// Generated message types from contracts.yaml
pub mod generated;

pub use envelope::{Envelope, EnvelopeError};
pub use fixed_vec::{FixedStr, FixedVec, FixedVecError};
pub use message::Message;

// Export generated types as the primary API
pub use generated::{ArbitrageSignal, InstrumentMeta, PoolStateUpdate, ValidationError, U256};
