//! Transport-specific tests
//!
//! Tests for individual transport implementations:
//! - Local (Arc<T>) transport
//! - Unix socket transport
//! - TCP socket transport

pub mod bridge_handshake;
pub mod local_transport;
pub mod ocaml_bridge_service;
pub mod python_bridge_service;
// TODO: Re-enable these tests after migration
// pub mod unix_transport;
// pub mod tcp_transport;
// pub mod transport_fallbacks;
