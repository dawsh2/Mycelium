//! Transport-specific tests
//!
//! Tests for individual transport implementations:
//! - Local (Arc<T>) transport
//! - Unix socket transport
//! - TCP socket transport

pub mod local_transport;
// TODO: Re-enable these tests after migration
// pub mod unix_transport;
// pub mod tcp_transport;
// pub mod transport_fallbacks;