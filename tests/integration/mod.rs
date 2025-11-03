//! Integration tests organized by deployment mode
//!
//! Tests cover all transport types and deployment scenarios:
//! - Monolith: All services in one process (Arc<T> only)
//! - Bundled: Services grouped in bundles (Arc + Unix/TCP)
//! - Distributed: Services across machines (TCP only)

pub mod monolith;
pub mod bundled;
pub mod distributed;
pub mod mixed;
pub mod error_handling;

// Common test utilities
pub use mycelium_protocol::impl_message;
pub use mycelium_transport::MessageBus;
pub use rkyv::Archive;
pub use zerocopy::{AsBytes, FromBytes, FromZeroes};

// Test message types
#[derive(Debug, Clone, Copy, PartialEq, Archive, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
pub struct SwapEvent {
    pub pool_id: u64,
    pub amount_in: u128,
    pub amount_out: u128,
    pub timestamp: u64,
}

impl_message!(SwapEvent, 1, "market-data");

#[derive(Debug, Clone, Copy, PartialEq, Archive, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
pub struct ArbitrageSignal {
    pub opportunity_id: u64,
    pub profit: u128,
    pub timestamp: u64,
}

impl_message!(ArbitrageSignal, 2, "arbitrage");

#[derive(Debug, Clone, Copy, PartialEq, Archive, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
pub struct OrderExecution {
    pub order_id: u64,
    pub success: bool,
    pub timestamp: u64,
}

impl_message!(OrderExecution, 3, "orders");