//! Shared transport utilities and common functionality
//!
//! This module contains code that's shared across different transport implementations
//! to reduce duplication and ensure consistent behavior.

use crate::config::TransportConfig;
use crate::{Result, TransportError};
use dashmap::DashMap;
use mycelium_protocol::{Envelope, Message};
use std::sync::Arc;
use tokio::sync::broadcast;

/// Common broadcast channel management for all transports
#[derive(Clone)]
pub struct ChannelManager {
    /// Topic -> broadcast channel mapping
    channels: Arc<DashMap<String, broadcast::Sender<Envelope>>>,
    /// Transport configuration
    config: TransportConfig,
}

impl ChannelManager {
    pub fn new(config: TransportConfig) -> Self {
        Self {
            channels: Arc::new(DashMap::new()),
            config,
        }
    }

    /// Get or create a broadcast channel for a topic (from Message trait)
    pub fn get_or_create_channel<M: Message>(&self) -> broadcast::Sender<Envelope> {
        let topic = M::TOPIC;
        self.get_or_create_channel_for_topic(topic)
    }

    /// Get or create a broadcast channel for an explicit topic string (Phase 1: Actor-ready)
    ///
    /// This enables dynamic topic creation for actor mailboxes and partitioned topics.
    pub fn get_or_create_channel_for_topic(&self, topic: &str) -> broadcast::Sender<Envelope> {
        self.channels
            .entry(topic.to_string())
            .or_insert_with(|| broadcast::channel(self.config.channel_capacity).0)
            .clone()
    }

    /// Get the number of active topics
    pub fn topic_count(&self) -> usize {
        self.channels.len()
    }

    /// Get subscriber count for a topic
    pub fn subscriber_count<M: Message>(&self) -> usize {
        let topic = M::TOPIC;
        self.channels
            .get(topic)
            .map(|entry| entry.receiver_count())
            .unwrap_or(0)
    }

    /// Remove unused channels (cleanup)
    pub fn cleanup_unused_channels(&self) {
        self.channels.retain(|_, entry| entry.receiver_count() > 0);
    }
}

/// Common connection management for network transports
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub target: String,
    pub transport_type: TransportType,
    pub created_at: std::time::Instant,
    pub last_used: std::time::Instant,
    pub messages_sent: u64,
}

// TransportType moved to config module
pub use crate::config::TransportType;

/// Common error context enhancement
pub fn enrich_error(error: TransportError, context: &str) -> TransportError {
    match error {
        TransportError::ServiceNotFound(service) => {
            TransportError::ServiceNotFound(format!("{}: {}", context, service))
        }
        TransportError::Io(io_err) => TransportError::Io(io_err),
        other => other,
    }
}

/// Common timeout handling
pub async fn with_timeout<F, T>(
    duration: std::time::Duration,
    future: F,
) -> std::result::Result<T, TransportError>
where
    F: std::future::Future<Output = Result<T>>,
{
    match tokio::time::timeout(duration, future).await {
        Ok(result) => result,
        Err(_) => Err(TransportError::Timeout(duration)),
    }
}

/// Common buffer size utilities
#[derive(Debug, Clone)]
pub struct BufferSizes {
    pub read_buffer: usize,
    pub write_buffer: usize,
    pub frame_buffer: usize,
    pub socket_buffer: usize,
    pub serialization_buffer: usize,
}

impl Default for BufferSizes {
    fn default() -> Self {
        Self {
            read_buffer: 64 * 1024,     // 64KB
            write_buffer: 64 * 1024,    // 64KB
            frame_buffer: 8 * 1024,     // 8KB
            socket_buffer: 128 * 1024,  // 128KB
            serialization_buffer: 1024, // 1KB
        }
    }
}

impl BufferSizes {
    pub fn from_config(config: &TransportConfig) -> Self {
        let base_size = config.channel_capacity.max(1024);
        Self {
            read_buffer: base_size * 64,
            write_buffer: base_size * 64,
            frame_buffer: base_size * 8,
            socket_buffer: base_size * 128,
            serialization_buffer: base_size.max(4096),
        }
    }

    pub fn for_high_throughput() -> Self {
        Self {
            read_buffer: 1024 * 1024,        // 1MB
            write_buffer: 1024 * 1024,       // 1MB
            frame_buffer: 256 * 1024,        // 256KB
            socket_buffer: 2 * 1024 * 1024,  // 2MB
            serialization_buffer: 64 * 1024, // 64KB
        }
    }

    pub fn for_low_latency() -> Self {
        Self {
            read_buffer: 16 * 1024,    // 16KB
            write_buffer: 16 * 1024,   // 16KB
            frame_buffer: 4 * 1024,    // 4KB
            socket_buffer: 32 * 1024,  // 32KB
            serialization_buffer: 256, // 256B
        }
    }

    pub fn for_memory_constrained() -> Self {
        Self {
            read_buffer: 8 * 1024,     // 8KB
            write_buffer: 8 * 1024,    // 8KB
            frame_buffer: 2 * 1024,    // 2KB
            socket_buffer: 16 * 1024,  // 16KB
            serialization_buffer: 128, // 128B
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.read_buffer < 1024 {
            return Err(TransportError::InvalidConfig(format!(
                "read_buffer ({}) must be at least 1024 bytes",
                self.read_buffer
            )));
        }
        if self.write_buffer < 1024 {
            return Err(TransportError::InvalidConfig(format!(
                "write_buffer ({}) must be at least 1024 bytes",
                self.write_buffer
            )));
        }
        if self.frame_buffer < 512 {
            return Err(TransportError::InvalidConfig(format!(
                "frame_buffer ({}) must be at least 512 bytes",
                self.frame_buffer
            )));
        }
        Ok(())
    }

    pub fn total_memory_usage(&self) -> usize {
        self.read_buffer
            + self.write_buffer
            + self.frame_buffer
            + self.socket_buffer
            + self.serialization_buffer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_sizes_default() {
        let sizes = BufferSizes::default();
        assert_eq!(sizes.read_buffer, 64 * 1024);
        assert_eq!(sizes.write_buffer, 64 * 1024);
        assert_eq!(sizes.frame_buffer, 8 * 1024);
        assert_eq!(sizes.socket_buffer, 128 * 1024);
        assert_eq!(sizes.serialization_buffer, 1024);
    }

    #[test]
    fn test_buffer_sizes_high_throughput() {
        let sizes = BufferSizes::for_high_throughput();
        assert_eq!(sizes.read_buffer, 1024 * 1024);
        assert_eq!(sizes.write_buffer, 1024 * 1024);
        assert!(sizes.total_memory_usage() > 3 * 1024 * 1024);
    }

    #[test]
    fn test_buffer_sizes_low_latency() {
        let sizes = BufferSizes::for_low_latency();
        assert_eq!(sizes.read_buffer, 16 * 1024);
        assert_eq!(sizes.serialization_buffer, 256);
    }

    #[test]
    fn test_buffer_sizes_memory_constrained() {
        let sizes = BufferSizes::for_memory_constrained();
        assert_eq!(sizes.read_buffer, 8 * 1024);
        assert_eq!(sizes.serialization_buffer, 128);
        assert!(sizes.total_memory_usage() < 40 * 1024);
    }

    #[test]
    fn test_buffer_sizes_validate_success() {
        let sizes = BufferSizes::default();
        assert!(sizes.validate().is_ok());

        let sizes = BufferSizes::for_low_latency();
        assert!(sizes.validate().is_ok());
    }

    #[test]
    fn test_buffer_sizes_validate_read_buffer_too_small() {
        let sizes = BufferSizes {
            read_buffer: 512, // Too small
            write_buffer: 2048,
            frame_buffer: 1024,
            socket_buffer: 4096,
            serialization_buffer: 256,
        };
        let result = sizes.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("read_buffer"));
    }

    #[test]
    fn test_buffer_sizes_validate_write_buffer_too_small() {
        let sizes = BufferSizes {
            read_buffer: 2048,
            write_buffer: 512, // Too small
            frame_buffer: 1024,
            socket_buffer: 4096,
            serialization_buffer: 256,
        };
        let result = sizes.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("write_buffer"));
    }

    #[test]
    fn test_buffer_sizes_validate_frame_buffer_too_small() {
        let sizes = BufferSizes {
            read_buffer: 2048,
            write_buffer: 2048,
            frame_buffer: 256, // Too small
            socket_buffer: 4096,
            serialization_buffer: 256,
        };
        let result = sizes.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("frame_buffer"));
    }

    #[test]
    fn test_buffer_sizes_total_memory() {
        let sizes = BufferSizes {
            read_buffer: 1000,
            write_buffer: 2000,
            frame_buffer: 500,
            socket_buffer: 3000,
            serialization_buffer: 500,
        };
        assert_eq!(sizes.total_memory_usage(), 7000);
    }

    #[tokio::test]
    async fn test_with_timeout_success() {
        let future = async { Ok::<i32, TransportError>(42) };
        let result = with_timeout(std::time::Duration::from_secs(1), future).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_with_timeout_expires() {
        let future = async {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            Ok::<i32, TransportError>(42)
        };
        let result = with_timeout(std::time::Duration::from_millis(10), future).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TransportError::Timeout(_)));
    }

    #[tokio::test]
    async fn test_with_timeout_inner_error() {
        let future = async { Err::<i32, TransportError>(TransportError::SendFailed) };
        let result = with_timeout(std::time::Duration::from_secs(1), future).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TransportError::SendFailed));
    }

    #[test]
    fn test_enrich_error() {
        let err = TransportError::ServiceNotFound("my_service".to_string());
        let enriched = enrich_error(err, "context_info");
        assert_eq!(
            enriched.to_string(),
            "Service not found: context_info: my_service"
        );

        let err = TransportError::SendFailed;
        let enriched = enrich_error(err, "context");
        assert!(matches!(enriched, TransportError::SendFailed));
    }

    #[test]
    fn test_buffer_sizes_from_config() {
        let config = TransportConfig::default();
        let sizes = BufferSizes::from_config(&config);

        assert!(sizes.read_buffer >= 1024);
        assert!(sizes.validate().is_ok());
    }
}
