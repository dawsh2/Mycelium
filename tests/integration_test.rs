//! Integration test runner
//!
//! This module serves as the main integration test entry point.
//! Individual test modules are organized in the integration/ directory.

mod integration;

// Re-export all integration tests
use integration::*;