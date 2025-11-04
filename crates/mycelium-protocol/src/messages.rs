//! Core TLV message types
//!
//! Implements the message contracts defined in contracts.yaml.
//! All types use rkyv for zero-copy serialization.
//!
//! These are generic test messages for demonstration only.
//! Real applications should define their own contracts.yaml.

use crate::Message;
use rkyv::{Archive, Deserialize, Serialize};

// Note: Domain-specific messages (DeFi, trading, etc.) belong in application code,
// not in the mycelium framework itself. See examples/domain_specific/ for guidance.

// The actual message implementations will be generated from contracts.yaml
// by the build script. This file just provides the module structure.

// Re-exports for convenience (if needed by applications)
pub use primitive_types::U256;
