//! Backpressure handling utilities
//!
//! Provides sophisticated backpressure management for high-throughput scenarios
//! with configurable strategies and monitoring.

use crate::config::TransportConfig;
use crate::Result;
use mycelium_protocol::Message;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc, Semaphore};
use tokio::time::timeout;

/// Backpressure strategy configuration
#[derive(Debug, Clone)]
pub struct BackpressureConfig {
    /// Maximum number of in-flight messages
    pub max_in_flight: usize,
    /// Maximum queue size before applying backpressure
    pub max_queue_size: usize,
    /// Time to wait for queue space before failing
    pub queue_timeout: Duration,
    /// Whether to drop oldest messages when queue is full
    pub drop_oldest: bool,
    /// Flow control window size
    pub flow_control_window: usize,
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            max_in_flight: 1000,
            max_queue_size: 5000,
            queue_timeout: Duration::from_millis(100),
            drop_oldest: false,
            flow_control_window: 100,
        }
    }
}

impl BackpressureConfig {
    pub fn from_transport_config(config: &TransportConfig) -> Self {
        Self {
            max_in_flight: config.channel_capacity,
            max_queue_size: config.channel_capacity * 5,
            queue_timeout: Duration::from_millis(50),
            drop_oldest: false,
            flow_control_window: config.channel_capacity / 10,
        }
    }

    pub fn for_high_throughput() -> Self {
        Self {
            max_in_flight: 10000,
            max_queue_size: 50000,
            queue_timeout: Duration::from_millis(10),
            drop_oldest: true,
            flow_control_window: 1000,
        }
    }

    pub fn for_low_latency() -> Self {
        Self {
            max_in_flight: 100,
            max_queue_size: 500,
            queue_timeout: Duration::from_millis(1),
            drop_oldest: true,
            flow_control_window: 10,
        }
    }
}

/// Backpressure statistics
#[derive(Debug)]
pub struct BackpressureStats {
    pub messages_sent: AtomicU64,
    pub messages_dropped: AtomicU64,
    pub messages_queued: AtomicU64,
    pub current_queue_size: AtomicUsize,
    pub max_queue_size_seen: AtomicUsize,
    pub avg_queue_latency: Duration,
    pub backpressure_applied_count: AtomicU64,
}

impl BackpressureStats {
    pub fn new() -> Self {
        Self {
            messages_sent: AtomicU64::new(0),
            messages_dropped: AtomicU64::new(0),
            messages_queued: AtomicU64::new(0),
            current_queue_size: AtomicUsize::new(0),
            max_queue_size_seen: AtomicUsize::new(0),
            avg_queue_latency: Duration::ZERO,
            backpressure_applied_count: AtomicU64::new(0),
        }
    }
}

impl std::fmt::Display for BackpressureStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Backpressure: {} sent, {} dropped, {} queued, {}/{} current/max queue, avg latency: {:?}",
            self.messages_sent.load(Ordering::Relaxed),
            self.messages_dropped.load(Ordering::Relaxed),
            self.messages_queued.load(Ordering::Relaxed),
            self.current_queue_size.load(Ordering::Relaxed),
            self.max_queue_size_seen.load(Ordering::Relaxed),
            self.avg_queue_latency
        )
    }
}

/// Enhanced sender with backpressure handling
pub struct BackpressuredSender<T> {
    sender: mpsc::Sender<T>,
    config: BackpressureConfig,
    stats: Arc<BackpressureStats>,
    in_flight: Arc<Semaphore>,
    queue_times: Arc<std::sync::Mutex<Vec<Duration>>>,
}

impl<T> BackpressuredSender<T> {
    pub fn new(config: BackpressureConfig) -> (Self, mpsc::Receiver<T>) {
        let (sender, receiver) = mpsc::channel(config.max_queue_size);
        let stats = Arc::new(BackpressureStats::new());
        let in_flight = Arc::new(Semaphore::new(config.max_in_flight));
        let queue_times = Arc::new(std::sync::Mutex::new(Vec::new()));

        let sender = Self {
            sender,
            config,
            stats: Arc::clone(&stats),
            in_flight,
            queue_times,
        };

        (sender, receiver)
    }

    /// Send a message with backpressure handling
    pub async fn send(&self, item: T) -> Result<()> {
        let start_time = Instant::now();

        // Check in-flight limit
        let _permit = timeout(self.config.queue_timeout, self.in_flight.acquire())
            .await
            .map_err(|_| crate::TransportError::SendFailed)? // TODO: Add Timeout error
            .map_err(|_| crate::TransportError::SendFailed)?; // Semaphore closed

        // Try to send with timeout
        let send_result = timeout(self.config.queue_timeout, self.sender.send(item)).await;

        match send_result {
            Ok(Ok(())) => {
                // Record statistics
                let elapsed = start_time.elapsed();
                self.record_send(elapsed, true).await;
                Ok(())
            }
            Ok(Err(_)) => {
                // Channel closed
                self.record_send(start_time.elapsed(), false).await;
                Err(crate::TransportError::SendFailed)
            }
            Err(_) => {
                // Timeout - apply backpressure strategy
                self.handle_backpressure_timeout(start_time.elapsed()).await
            }
        }
    }

    /// Try to send without blocking
    pub fn try_send(&self, item: T) -> Result<()> {
        // Check in-flight limit
        if self.in_flight.available_permits() == 0 {
            self.stats.messages_dropped.fetch_add(1, Ordering::Relaxed);
            return Err(crate::TransportError::SendFailed); // TODO: Add specific error
        }

        match self.sender.try_send(item) {
            Ok(()) => {
                self.stats.messages_sent.fetch_add(1, Ordering::Relaxed);
                self.stats.messages_queued.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                if self.config.drop_oldest {
                    // Drop oldest message and try again
                    if let Ok(_) = self.sender.try_reserve() {
                        let _ = self.sender.try_send(item);
                        self.stats.messages_dropped.fetch_add(1, Ordering::Relaxed);
                        self.stats.messages_sent.fetch_add(1, Ordering::Relaxed);
                        Ok(())
                    } else {
                        self.stats.messages_dropped.fetch_add(1, Ordering::Relaxed);
                        Err(crate::TransportError::SendFailed)
                    }
                } else {
                    self.stats.messages_dropped.fetch_add(1, Ordering::Relaxed);
                    Err(crate::TransportError::SendFailed)
                }
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                Err(crate::TransportError::SendFailed)
            }
        }
    }

    async fn record_send(&self, latency: Duration, success: bool) {
        if success {
            self.stats.messages_sent.fetch_add(1, Ordering::Relaxed);
            self.stats.messages_queued.fetch_add(1, Ordering::Relaxed);

            // Update queue latency statistics (keep last 100 samples)
            let mut queue_times = self.queue_times.lock().unwrap();
            queue_times.push(latency);
            if queue_times.len() > 100 {
                queue_times.remove(0);
            }

            // Calculate average
            if !queue_times.is_empty() {
                let sum: Duration = queue_times.iter().sum();
                let count = queue_times.len() as u32;
                let avg = sum / count;
                // Update atomic average (approximation)
                self.stats.avg_queue_latency = avg;
            }
        } else {
            self.stats.messages_dropped.fetch_add(1, Ordering::Relaxed);
        }

        // Update queue size tracking
        let current_size = self.sender.len();
        self.stats.current_queue_size = current_size;

        let max_seen = self.stats.max_queue_size_seen;
        if current_size > max_seen {
            self.stats.max_queue_size_seen = current_size;
        }
    }

    async fn handle_backpressure_timeout(&self, latency: Duration) -> Result<()> {
        self.stats.backpressure_applied_count.fetch_add(1, Ordering::Relaxed);

        if self.config.drop_oldest {
            // Drop oldest message and try again
            if let Ok(_) = timeout(Duration::from_millis(1), self.sender.reserve()).await {
                // This would need the item to resend - for now, just fail
                self.record_send(latency, false).await;
                Err(crate::TransportError::SendFailed)
            } else {
                self.record_send(latency, false).await;
                Err(crate::TransportError::SendFailed)
            }
        } else {
            // Just fail the send
            self.record_send(latency, false).await;
            Err(crate::TransportError::SendFailed)
        }
    }

    /// Get current statistics
    pub fn get_stats(&self) -> BackpressureStats {
        BackpressureStats {
            messages_sent: self.stats.messages_sent.load(Ordering::Relaxed),
            messages_dropped: self.stats.messages_dropped.load(Ordering::Relaxed),
            messages_queued: self.stats.messages_queued.load(Ordering::Relaxed),
            current_queue_size: self.sender.len(),
            max_queue_size_seen: self.stats.max_queue_size_seen,
            avg_queue_latency: self.stats.avg_queue_latency,
            backpressure_applied_count: self.stats.backpressure_applied_count.load(Ordering::Relaxed),
        }
    }
}

/// Enhanced receiver with flow control
pub struct BackpressuredReceiver<T> {
    receiver: mpsc::Receiver<T>,
    flow_control_window: usize,
    messages_received: AtomicU64,
}

impl<T> BackpressuredReceiver<T> {
    pub fn new(receiver: mpsc::Receiver<T>, flow_control_window: usize) -> Self {
        Self {
            receiver,
            flow_control_window,
            messages_received: AtomicU64::new(0),
        }
    }

    pub async fn recv(&mut self) -> Option<T> {
        let msg = self.receiver.recv().await;
        if msg.is_some() {
            self.messages_received.fetch_add(1, Ordering::Relaxed);
        }
        msg
    }

    pub fn try_recv(&mut self) -> Option<T> {
        let msg = self.receiver.try_recv().ok();
        if msg.is_some() {
            self.messages_received.fetch_add(1, Ordering::Relaxed);
        }
        msg
    }

    pub fn messages_received(&self) -> u64 {
        self.messages_received.load(Ordering::Relaxed)
    }

    pub fn len(&self) -> usize {
        self.receiver.len()
    }
}

/// Flow control mechanism for regulating message flow
#[derive(Debug)]
pub struct FlowController {
    window_size: usize,
    current_window: AtomicUsize,
    total_sent: AtomicU64,
    total_received: AtomicU64,
}

impl FlowController {
    pub fn new(window_size: usize) -> Self {
        Self {
            window_size,
            current_window: AtomicUsize::new(0),
            total_sent: AtomicU64::new(0),
            total_received: AtomicU64::new(0),
        }
    }

    pub fn can_send(&self) -> bool {
        self.current_window.load(Ordering::Relaxed) < self.window_size
    }

    pub fn record_send(&self) -> bool {
        let current = self.current_window.fetch_add(1, Ordering::Relaxed);
        self.total_sent.fetch_add(1, Ordering::Relaxed);
        current < self.window_size - 1
    }

    pub fn record_receive(&self) {
        let current = self.current_window.fetch_sub(1, Ordering::Relaxed);
        self.total_received.fetch_add(1, Ordering::Relaxed);
        // Prevent underflow
        if current == 0 {
            self.current_window.store(0, Ordering::Relaxed);
        }
    }

    pub fn get_stats(&self) -> FlowControlStats {
        FlowControlStats {
            window_size: self.window_size,
            current_window: self.current_window.load(Ordering::Relaxed),
            total_sent: self.total_sent.load(Ordering::Relaxed),
            total_received: self.total_received.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FlowControlStats {
    pub window_size: usize,
    pub current_window: usize,
    pub total_sent: u64,
    pub total_received: u64,
}

impl std::fmt::Display for FlowControlStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "FlowControl: {}/{} in window, {} sent, {} received",
            self.current_window, self.window_size, self.total_sent, self.total_received
        )
    }
}