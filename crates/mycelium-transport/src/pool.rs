//! Connection pooling for network transports
//!
//! Provides connection reuse and management for TCP and Unix transports
//! to improve performance and reduce connection overhead.

use crate::config::{ConnectionPoolConfig, TransportConfig};
use crate::{Result, TransportError, TransportType};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpStream, UnixStream};
use tokio::sync::{Mutex, RwLock};
use tokio::time::interval;

/// A pooled connection with metadata
#[derive(Debug)]
pub struct PooledConnection<T> {
    /// The underlying connection
    pub stream: T,
    /// When this connection was created
    pub created_at: Instant,
    /// When this connection was last used
    pub last_used: Arc<Mutex<Instant>>,
    /// Number of messages sent through this connection
    pub message_count: Arc<Mutex<u64>>,
    /// Whether the connection is healthy
    pub healthy: Arc<RwLock<bool>>,
}

impl<T> PooledConnection<T> {
    pub fn new(stream: T) -> Self {
        let now = Instant::now();
        Self {
            stream,
            created_at: now,
            last_used: Arc::new(Mutex::new(now)),
            message_count: Arc::new(Mutex::new(0)),
            healthy: Arc::new(RwLock::new(true)),
        }
    }

    pub async fn record_usage(&self) {
        let mut last_used = self.last_used.lock().await;
        *last_used = Instant::now();

        let mut count = self.message_count.lock().await;
        *count += 1;
    }

    pub async fn is_healthy(&self) -> bool {
        *self.healthy.read().await
    }

    pub async fn mark_unhealthy(&self) {
        let mut healthy = self.healthy.write().await;
        *healthy = false;
    }

    pub async fn get_message_count(&self) -> u64 {
        *self.message_count.lock().await
    }

    pub async fn get_idle_time(&self) -> Duration {
        let last_used = *self.last_used.lock().await;
        Instant::now().duration_since(last_used)
    }
}

/// Connection pool for network transports
#[derive(Debug)]
pub struct ConnectionPool<T> {
    /// Pool of active connections
    connections: Arc<RwLock<HashMap<String, Vec<PooledConnection<T>>>>>,
    /// Pool configuration
    config: ConnectionPoolConfig,
    /// Target identifier for this pool
    target: String,
    /// Cleanup task handle
    _cleanup_handle: tokio::task::JoinHandle<()>,
}

impl<T> ConnectionPool<T>
where
    T: Clone + Send + Sync + 'static,
{
    pub fn new(target: String, config: ConnectionPoolConfig) -> Self {
        let connections = Arc::new(RwLock::new(HashMap::new()));

        let cleanup_handle = if config.enabled {
            let cleanup_connections = connections.clone();
            let cleanup_config = config.clone();
            let cleanup_target = target.clone();

            tokio::spawn(async move {
                let mut interval = interval(Duration::from_secs(cleanup_config.idle_timeout_secs / 10));

                loop {
                    interval.tick().await;
                    Self::cleanup_idle_connections(&cleanup_connections, &cleanup_config).await;
                }
            })
        } else {
            tokio::spawn(async {})
        };

        Self {
            connections,
            config,
            target,
            _cleanup_handle: cleanup_handle,
        }
    }

    /// Get a connection from the pool or create a new one
    pub async fn get_connection<F, Fut>(&self, create_connection: F) -> Result<PooledConnection<T>>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        if !self.config.enabled {
            // Pooling disabled - create new connection each time
            let stream = create_connection().await?;
            return Ok(PooledConnection::new(stream));
        }

        let mut connections = self.connections.write().await;
        let pool = connections.entry(self.target.clone()).or_insert_with(Vec::new);

        // Try to find a healthy connection
        while let Some(mut conn) = pool.pop() {
            if conn.is_healthy().await {
                conn.record_usage().await;
                return Ok(conn);
            }
            // Drop unhealthy connection
        }

        // No healthy connections available - create new one
        let stream = create_connection().await?;
        Ok(PooledConnection::new(stream))
    }

    /// Return a connection to the pool
    pub async fn return_connection(&self, connection: PooledConnection<T>) -> bool {
        if !self.config.enabled {
            return false; // Pooling disabled
        }

        // Check if connection is still healthy
        if !connection.is_healthy().await {
            return false;
        }

        // Check if pool has space
        let mut connections = self.connections.write().await;
        let pool = connections.entry(self.target.clone()).or_insert_with(Vec::new);

        if pool.len() < self.config.max_connections {
            pool.push(connection);
            true
        } else {
            false // Pool full
        }
    }

    /// Close all connections in the pool
    pub async fn close_all(&self) {
        let mut connections = self.connections.write().await;
        connections.clear();
    }

    /// Get pool statistics
    pub async fn get_stats(&self) -> PoolStats {
        let connections = self.connections.read().await;
        let pool = connections.get(&self.target);

        let (total, healthy, total_messages) = if let Some(pool) = pool {
            let mut healthy_count = 0;
            let mut total_messages = 0;

            for conn in pool {
                if conn.is_healthy().await {
                    healthy_count += 1;
                }
                total_messages += conn.get_message_count().await;
            }

            (pool.len(), healthy_count, total_messages)
        } else {
            (0, 0, 0)
        };

        PoolStats {
            target: self.target.clone(),
            total_connections: total,
            healthy_connections: healthy,
            total_messages_sent: total_messages,
            max_connections: self.config.max_connections,
            pooling_enabled: self.config.enabled,
        }
    }

    async fn cleanup_idle_connections(
        connections: &Arc<RwLock<HashMap<String, Vec<PooledConnection<T>>>>>,
        config: &ConnectionPoolConfig,
    ) {
        let mut connections = connections.write().await;
        let idle_timeout = Duration::from_secs(config.idle_timeout_secs);

        for (_, pool) in connections.iter_mut() {
            pool.retain(|conn| {
                let idle_time = futures::executor::block_on(conn.get_idle_time());
                let is_healthy = futures::executor::block_on(conn.is_healthy());
                idle_time < idle_timeout && is_healthy
            });
        }
    }
}

#[derive(Debug, Clone)]
pub struct PoolStats {
    pub target: String,
    pub total_connections: usize,
    pub healthy_connections: usize,
    pub total_messages_sent: u64,
    pub max_connections: usize,
    pub pooling_enabled: bool,
}

impl std::fmt::Display for PoolStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Pool {}: {}/{} connections ({} healthy), {} messages sent, pooling: {}",
            self.target,
            self.total_connections,
            self.max_connections,
            self.healthy_connections,
            self.total_messages_sent,
            self.pooling_enabled
        )
    }
}

/// TCP connection pool
pub type TcpConnectionPool = ConnectionPool<TcpStream>;

/// Unix connection pool
pub type UnixConnectionPool = ConnectionPool<UnixStream>;

impl TcpConnectionPool {
    pub fn for_address(addr: SocketAddr, config: ConnectionPoolConfig) -> Self {
        Self::new(addr.to_string(), config)
    }
}

impl UnixConnectionPool {
    pub fn for_path(path: PathBuf, config: ConnectionPoolConfig) -> Self {
        Self::new(path.display().to_string(), config)
    }
}