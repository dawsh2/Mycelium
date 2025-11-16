//! Simple metrics tracking for services
//!
//! This provides basic metrics collection without external dependencies.
//! In production, these could be exported to Prometheus, StatsD, etc.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Metrics collected for a service
#[derive(Clone)]
pub struct ServiceMetrics {
    inner: Arc<ServiceMetricsInner>,
}

struct ServiceMetricsInner {
    /// Total number of messages emitted
    emits_total: AtomicU64,

    /// Total emit latency in microseconds
    emit_latency_us_total: AtomicU64,

    /// Number of errors encountered
    errors_total: AtomicU64,

    /// Number of service restarts
    restarts_total: AtomicU64,

    /// Custom per-service counter metrics
    custom_counters: Mutex<HashMap<&'static str, u64>>,
}

impl ServiceMetrics {
    /// Create a new metrics collector
    pub fn new() -> Self {
        Self {
            inner: Arc::new(ServiceMetricsInner {
                emits_total: AtomicU64::new(0),
                emit_latency_us_total: AtomicU64::new(0),
                errors_total: AtomicU64::new(0),
                restarts_total: AtomicU64::new(0),
                custom_counters: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Record a message emission
    pub fn record_emit(&self, latency_us: u64) {
        self.inner.emits_total.fetch_add(1, Ordering::Relaxed);
        self.inner
            .emit_latency_us_total
            .fetch_add(latency_us, Ordering::Relaxed);
    }

    /// Record an error
    pub fn record_error(&self) {
        self.inner.errors_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a service restart
    pub fn record_restart(&self) {
        self.inner.restarts_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Get total number of emits
    pub fn emits_total(&self) -> u64 {
        self.inner.emits_total.load(Ordering::Relaxed)
    }

    /// Get average emit latency in microseconds
    pub fn emit_latency_us_avg(&self) -> u64 {
        let total = self.inner.emits_total.load(Ordering::Relaxed);
        if total == 0 {
            return 0;
        }
        let latency_total = self.inner.emit_latency_us_total.load(Ordering::Relaxed);
        latency_total / total
    }

    /// Get total number of errors
    pub fn errors_total(&self) -> u64 {
        self.inner.errors_total.load(Ordering::Relaxed)
    }

    /// Get total number of restarts
    pub fn restarts_total(&self) -> u64 {
        self.inner.restarts_total.load(Ordering::Relaxed)
    }

    /// Increment a custom counter for the service
    pub fn incr_counter(&self, name: &'static str, delta: u64) {
        if delta == 0 {
            return;
        }
        let mut counters = self
            .inner
            .custom_counters
            .lock()
            .expect("custom counters lock");
        *counters.entry(name).or_insert(0) += delta;
    }

    /// Fetch the current value of a custom counter, if present
    pub fn counter(&self, name: &'static str) -> Option<u64> {
        let counters = self
            .inner
            .custom_counters
            .lock()
            .expect("custom counters lock");
        counters.get(name).copied()
    }

    fn custom_counters_snapshot(&self) -> Vec<(String, u64)> {
        let counters = self
            .inner
            .custom_counters
            .lock()
            .expect("custom counters lock");
        counters
            .iter()
            .map(|(k, v)| ((*k).to_string(), *v))
            .collect()
    }

    /// Print metrics summary
    pub fn print_summary(&self, service_name: &str) {
        let custom_counters = self.custom_counters_snapshot();
        tracing::info!(
            service = service_name,
            emits_total = self.emits_total(),
            emit_latency_us_avg = self.emit_latency_us_avg(),
            errors_total = self.errors_total(),
            restarts_total = self.restarts_total(),
            custom_counters = ?custom_counters,
            "Service metrics summary"
        );
    }
}

impl Default for ServiceMetrics {
    fn default() -> Self {
        Self::new()
    }
}
