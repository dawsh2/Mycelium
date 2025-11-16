//! Deprecated compatibility shim for legacy `runtime` module.
//!
//! Prefer importing `ServiceRuntime`, `ServiceHandle`, and `Service` directly from
//! `mycelium_transport` (they now live in `service_runtime.rs`). This module is
//! retained temporarily to provide a softer migration path.

#[deprecated(
    since = "0.1.0",
    note = "Import ServiceRuntime from mycelium_transport (service_runtime.rs) instead"
)]
pub use crate::service_runtime::{Service, ServiceHandle, ServiceRuntime};
