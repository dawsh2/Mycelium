//! Deprecated compatibility shim for legacy `metrics` module.
//!
//! Use `mycelium_transport::ServiceMetrics` (from `service_metrics.rs`).

#[deprecated(
    since = "0.1.0",
    note = "Import ServiceMetrics from mycelium_transport::service_metrics instead"
)]
pub use crate::service_metrics::ServiceMetrics;
