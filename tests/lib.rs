//! Mycelium Integration Test Suite
//!
//! Organized test suite covering all aspects of the Mycelium messaging system:
//! - Basic functionality tests
//! - Transport-specific tests
//! - Deployment mode tests
//! - Performance and stress tests
//! - Error handling and edge cases

pub mod common;
pub mod deployment_tests;
pub mod performance_tests;
pub mod transport_tests;
// TODO: Re-enable after migration - error handling tests exist in integration/error_handling.rs
// pub mod error_handling_tests;

// Re-export common utilities for easier access
pub use common::*;
