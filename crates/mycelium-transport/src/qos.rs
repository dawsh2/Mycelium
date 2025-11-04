//! Quality of Service (QoS) and backpressure policies
//!
//! Provides configuration for actor mailbox behavior, including:
//! - Drop policies for when mailboxes are full
//! - Capacity limits
//! - CPU affinity hints
//! - Metrics tracking

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Policy for handling messages when mailbox is full
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropPolicy {
    /// Drop the oldest message (ring buffer behavior)
    ///
    /// Best for: Real-time data where latest is most important (market data)
    DropOldest,

    /// Drop the newest message (keep historical data)
    ///
    /// Best for: When historical context matters more than latest updates
    DropNewest,

    /// Reject new messages and return error to sender
    ///
    /// Best for: Critical messages that must not be lost
    Reject,

    /// Block until space is available (backpressure)
    ///
    /// Best for: When sender can wait and backpressure is acceptable
    Block,
}

impl Default for DropPolicy {
    fn default() -> Self {
        // Default to blocking for safety
        DropPolicy::Block
    }
}

/// Configuration options for spawning actors
#[derive(Debug, Clone)]
pub struct SpawnOptions {
    /// Maximum mailbox capacity (None = unbounded)
    pub mailbox_capacity: Option<usize>,

    /// Policy for when mailbox is full
    pub drop_policy: DropPolicy,

    /// CPU affinity hints (core IDs to pin to)
    pub cpu_affinity: Option<Vec<usize>>,

    /// Enable metrics collection
    pub enable_metrics: bool,
}

impl Default for SpawnOptions {
    fn default() -> Self {
        Self {
            mailbox_capacity: Some(1000), // Bounded by default
            drop_policy: DropPolicy::Block,
            cpu_affinity: None,
            enable_metrics: true,
        }
    }
}

impl SpawnOptions {
    /// Create spawn options with unbounded mailbox
    pub fn unbounded() -> Self {
        Self {
            mailbox_capacity: None,
            ..Default::default()
        }
    }

    /// Create spawn options with specific capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            mailbox_capacity: Some(capacity),
            ..Default::default()
        }
    }

    /// Set the drop policy
    pub fn with_drop_policy(mut self, policy: DropPolicy) -> Self {
        self.drop_policy = policy;
        self
    }

    /// Set CPU affinity
    pub fn with_cpu_affinity(mut self, cores: Vec<usize>) -> Self {
        self.cpu_affinity = Some(cores);
        self
    }

    /// Enable or disable metrics
    pub fn with_metrics(mut self, enable: bool) -> Self {
        self.enable_metrics = enable;
        self
    }
}

/// Mailbox metrics for monitoring and observability
#[derive(Debug, Default)]
pub struct MailboxMetrics {
    /// Total messages dropped due to full mailbox
    pub dropped_messages: AtomicU64,

    /// Current queue depth
    pub queue_depth: AtomicUsize,

    /// Maximum latency observed (microseconds)
    pub max_latency_us: AtomicU64,

    /// Total messages processed
    pub total_processed: AtomicU64,

    /// Total messages rejected (when using Reject policy)
    pub total_rejected: AtomicU64,
}

impl MailboxMetrics {
    /// Create new metrics
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a dropped message
    pub fn record_drop(&self) {
        self.dropped_messages.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a rejected message
    pub fn record_reject(&self) {
        self.total_rejected.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a processed message
    pub fn record_processed(&self) {
        self.total_processed.fetch_add(1, Ordering::Relaxed);
    }

    /// Update queue depth
    pub fn set_queue_depth(&self, depth: usize) {
        self.queue_depth.store(depth, Ordering::Relaxed);
    }

    /// Update max latency
    pub fn update_max_latency(&self, latency_us: u64) {
        let mut current = self.max_latency_us.load(Ordering::Relaxed);
        while latency_us > current {
            match self.max_latency_us.compare_exchange_weak(
                current,
                latency_us,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    /// Get the number of dropped messages
    pub fn dropped(&self) -> u64 {
        self.dropped_messages.load(Ordering::Relaxed)
    }

    /// Get the current queue depth
    pub fn depth(&self) -> usize {
        self.queue_depth.load(Ordering::Relaxed)
    }

    /// Get the maximum latency in microseconds
    pub fn max_latency(&self) -> u64 {
        self.max_latency_us.load(Ordering::Relaxed)
    }

    /// Get the total messages processed
    pub fn processed(&self) -> u64 {
        self.total_processed.load(Ordering::Relaxed)
    }

    /// Get the total messages rejected
    pub fn rejected(&self) -> u64 {
        self.total_rejected.load(Ordering::Relaxed)
    }

    /// Get the drop rate (dropped / processed)
    pub fn drop_rate(&self) -> f64 {
        let processed = self.processed();
        if processed == 0 {
            0.0
        } else {
            self.dropped() as f64 / processed as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drop_policy_default() {
        let policy = DropPolicy::default();
        assert_eq!(policy, DropPolicy::Block);
    }

    #[test]
    fn test_spawn_options_default() {
        let opts = SpawnOptions::default();
        assert_eq!(opts.mailbox_capacity, Some(1000));
        assert_eq!(opts.drop_policy, DropPolicy::Block);
        assert!(opts.enable_metrics);
    }

    #[test]
    fn test_spawn_options_unbounded() {
        let opts = SpawnOptions::unbounded();
        assert_eq!(opts.mailbox_capacity, None);
    }

    #[test]
    fn test_spawn_options_builder() {
        let opts = SpawnOptions::with_capacity(500)
            .with_drop_policy(DropPolicy::DropOldest)
            .with_cpu_affinity(vec![0, 1])
            .with_metrics(false);

        assert_eq!(opts.mailbox_capacity, Some(500));
        assert_eq!(opts.drop_policy, DropPolicy::DropOldest);
        assert_eq!(opts.cpu_affinity, Some(vec![0, 1]));
        assert!(!opts.enable_metrics);
    }

    #[test]
    fn test_mailbox_metrics() {
        let metrics = MailboxMetrics::new();

        metrics.record_drop();
        metrics.record_drop();
        assert_eq!(metrics.dropped(), 2);

        metrics.record_reject();
        assert_eq!(metrics.rejected(), 1);

        metrics.record_processed();
        metrics.record_processed();
        metrics.record_processed();
        assert_eq!(metrics.processed(), 3);

        metrics.set_queue_depth(42);
        assert_eq!(metrics.depth(), 42);

        metrics.update_max_latency(100);
        assert_eq!(metrics.max_latency(), 100);

        metrics.update_max_latency(50); // Should not update (lower)
        assert_eq!(metrics.max_latency(), 100);

        metrics.update_max_latency(200); // Should update (higher)
        assert_eq!(metrics.max_latency(), 200);
    }

    #[test]
    fn test_drop_rate() {
        let metrics = MailboxMetrics::new();

        // No messages processed yet
        assert_eq!(metrics.drop_rate(), 0.0);

        // Process 10, drop 2
        for _ in 0..10 {
            metrics.record_processed();
        }
        metrics.record_drop();
        metrics.record_drop();

        assert_eq!(metrics.drop_rate(), 0.2); // 2/10 = 20%
    }
}
