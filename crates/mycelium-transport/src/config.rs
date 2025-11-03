use serde::{Deserialize, Serialize};

/// Configuration for all transport layers
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TransportConfig {
    /// Capacity for broadcast channels in all transports
    pub channel_capacity: usize,

    /// Connection pool settings for network transports
    pub connection_pool: ConnectionPoolConfig,

    /// Health check settings
    pub health_check: HealthCheckConfig,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            channel_capacity: 1000,
            connection_pool: ConnectionPoolConfig::default(),
            health_check: HealthCheckConfig::default(),
        }
    }
}

/// Connection pool configuration for TCP/Unix transports
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectionPoolConfig {
    /// Maximum number of connections to keep alive per target
    pub max_connections: usize,

    /// Idle timeout before closing unused connections (seconds)
    pub idle_timeout_secs: u64,

    /// Whether to enable connection pooling
    pub enabled: bool,
}

impl Default for ConnectionPoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 5,
            idle_timeout_secs: 300, // 5 minutes
            enabled: true,
        }
    }
}

/// Health check configuration for network transports
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HealthCheckConfig {
    /// Interval between health checks (seconds)
    pub interval_secs: u64,

    /// Timeout for health check responses (seconds)
    pub timeout_secs: u64,

    /// Maximum number of failed checks before marking as unhealthy
    pub max_failures: u32,

    /// Whether to enable health checks
    pub enabled: bool,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            interval_secs: 30,
            timeout_secs: 5,
            max_failures: 3,
            enabled: true,
        }
    }
}