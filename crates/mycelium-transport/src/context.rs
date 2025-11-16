//! Deprecated compatibility shim for legacy `context` module.
//!
//! Use `mycelium_transport::ServiceContext` (defined in `service_context.rs`).

#[deprecated(
    since = "0.1.0",
    note = "Import ServiceContext from mycelium_transport::service_context instead"
)]
pub use crate::service_context::ServiceContext;
