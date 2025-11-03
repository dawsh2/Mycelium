//! Deployment mode tests
//!
//! Tests for different deployment scenarios:
//! - Monolith deployment (single process)
//! - Bundled deployment (multiple bundles)
//! - Distributed deployment (multiple hosts)
//! - Mixed scenarios

pub mod monolith_deployment;
pub mod bundled_deployment;
pub mod distributed_deployment;
pub mod mixed_deployment;
pub mod topology_validation;