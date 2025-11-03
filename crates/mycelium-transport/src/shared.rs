//! Shared transport utilities and common functionality
//!
//! This module contains code that's shared across different transport implementations
//! to reduce duplication and ensure consistent behavior.

use crate::config::TransportConfig;
use crate::{TransportError, Result};
use dashmap::DashMap;
use mycelium_protocol::{Envelope, Message};
use std::sync::Arc;
use tokio::sync::broadcast;

/// Common broadcast channel management for all transports
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

    /// Get or create a broadcast channel for a topic
    pub fn get_or_create_channel<M: Message>(&self) -> broadcast::Sender<Envelope> {
        let topic = M::TOPIC;
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransportType {
    Local,
    Unix,
    Tcp,
}

/// Common error context enhancement
pub fn enrich_error(error: TransportError, context: &str) -> TransportError {
    match error {
        TransportError::ServiceNotFound(service) => {
            TransportError::ServiceNotFound(format!("{}: {}", context, service))
        }
        TransportError::Io(io_err) => {
            TransportError::Io(io_err)
        }
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
        Err(_) => Err(TransportError::SendFailed), // TODO: Add Timeout error variant
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
            read_buffer: 64 * 1024,      // 64KB
            write_buffer: 64 * 1024,     // 64KB
            frame_buffer: 8 * 1024,      // 8KB
            socket_buffer: 128 * 1024,   // 128KB
            serialization_buffer: 1024,   // 1KB
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
            read_buffer: 1024 * 1024,    // 1MB
            write_buffer: 1024 * 1024,   // 1MB
            frame_buffer: 256 * 1024,    // 256KB
            socket_buffer: 2 * 1024 * 1024, // 2MB
            serialization_buffer: 64 * 1024, // 64KB
        }
    }

    pub fn for_low_latency() -> Self {
        Self {
            read_buffer: 16 * 1024,       // 16KB
            write_buffer: 16 * 1024,      // 16KB
            frame_buffer: 4 * 1024,       // 4KB
            socket_buffer: 32 * 1024,     // 32KB
            serialization_buffer: 256,    // 256B
        }
    }

    pub fn for_memory_constrained() -> Self {
        Self {
            read_buffer: 8 * 1024,        // 8KB
            write_buffer: 8 * 1024,       // 8KB
            frame_buffer: 2 * 1024,       // 2KB
            socket_buffer: 16 * 1024,     // 16KB
            serialization_buffer: 128,    // 128B
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.read_buffer < 1024 {
            return Err(TransportError::SendFailed); // TODO: Add ConfigError
        }
        if self.write_buffer < 1024 {
            return Err(TransportError::SendFailed);
        }
        if self.frame_buffer < 512 {
            return Err(TransportError::SendFailed);
        }
        Ok(())
    }

    pub fn total_memory_usage(&self) -> usize {
        self.read_buffer + self.write_buffer + self.frame_buffer + self.socket_buffer + self.serialization_buffer
    }
}