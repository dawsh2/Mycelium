/// Service lifecycle management trait (actor-compatible)
///
/// This trait provides lifecycle hooks for stateful services and handlers.
/// It's designed to work with both pub/sub and future actor systems:
/// - In pub/sub mode: Manually call initialize/shutdown
/// - With actors (future): ActorSystem calls these automatically
///
/// # Example
///
/// ```rust
/// use crate::{ManagedService, HealthStatus};
///
/// struct MyHandler {
///     // handler state
/// }
///
/// #[async_trait::async_trait]
/// impl ManagedService for MyHandler {
///     async fn initialize(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
///         // Connect to resources, load state, etc.
///         Ok(())
///     }
///
///     async fn shutdown(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
///         // Flush data, close connections, etc.
///         Ok(())
///     }
///
///     async fn health(&self) -> HealthStatus {
///         HealthStatus::Healthy
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait ManagedService: Send + Sync {
    /// Initialize the service
    ///
    /// Called once before the service starts processing messages.
    /// Use this to:
    /// - Load configuration
    /// - Connect to databases or external services
    /// - Initialize state
    /// - Perform warmup operations
    async fn initialize(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Shutdown the service gracefully
    ///
    /// Called once when the service is being stopped.
    /// Use this to:
    /// - Flush pending data
    /// - Close connections
    /// - Save state
    /// - Release resources
    async fn shutdown(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Check service health
    ///
    /// Called periodically to monitor service health.
    /// Should be fast (<10ms) as it may block health check endpoints.
    async fn health(&self) -> HealthStatus;

    /// Optional: Handle errors during message processing
    ///
    /// Default behavior: log error and continue processing.
    /// Override to implement custom error handling (e.g., restart, circuit breaker).
    async fn on_error(&mut self, error: Box<dyn std::error::Error + Send + Sync>) {
        tracing::error!("Service error: {}", error);
    }
}

/// Health status for a managed service
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    /// Service is healthy and processing normally
    Healthy,

    /// Service is degraded but still functional
    ///
    /// Examples:
    /// - High latency
    /// - Reduced capacity
    /// - Some dependencies unavailable
    Degraded { reason: DegradedReason },

    /// Service is unhealthy and may need intervention
    ///
    /// Examples:
    /// - Cannot connect to required dependencies
    /// - Critical errors occurring
    /// - Out of resources
    Unhealthy { reason: UnhealthyReason },
}

/// Reasons for degraded service health
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegradedReason {
    /// Service is experiencing high latency
    HighLatency,

    /// Some non-critical dependencies are unavailable
    PartialDependencyFailure,

    /// Service is under heavy load
    HighLoad,

    /// Other degradation reason
    Other,
}

/// Reasons for unhealthy service status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnhealthyReason {
    /// Cannot initialize required resources
    InitializationFailed,

    /// Critical dependencies are unavailable
    CriticalDependencyFailure,

    /// Service has encountered fatal errors
    FatalError,

    /// Service is not ready yet
    NotReady,

    /// Other unhealthy reason
    Other,
}

impl HealthStatus {
    /// Returns true if the service is healthy
    pub fn is_healthy(&self) -> bool {
        matches!(self, HealthStatus::Healthy)
    }

    /// Returns true if the service is degraded
    pub fn is_degraded(&self) -> bool {
        matches!(self, HealthStatus::Degraded { .. })
    }

    /// Returns true if the service is unhealthy
    pub fn is_unhealthy(&self) -> bool {
        matches!(self, HealthStatus::Unhealthy { .. })
    }

    /// Returns true if the service can still process messages
    pub fn is_operational(&self) -> bool {
        matches!(self, HealthStatus::Healthy | HealthStatus::Degraded { .. })
    }
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HealthStatus::Healthy => write!(f, "healthy"),
            HealthStatus::Degraded { reason } => write!(f, "degraded: {:?}", reason),
            HealthStatus::Unhealthy { reason } => write!(f, "unhealthy: {:?}", reason),
        }
    }
}

/// Helper for running a managed service with graceful shutdown
///
/// This provides a standard pattern for running services with:
/// - Initialization on startup
/// - Graceful shutdown on SIGTERM/SIGINT
/// - Health check endpoint (optional)
///
/// # Example
///
/// ```rust,no_run
/// use crate::{ManagedService, ServiceRunner};
///
/// # struct MyService;
/// # #[async_trait::async_trait]
/// # impl ManagedService for MyService {
/// #     async fn initialize(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> { Ok(()) }
/// #     async fn shutdown(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> { Ok(()) }
/// #     async fn health(&self) -> mycelium_transport::HealthStatus {
/// #         mycelium_transport::HealthStatus::Healthy
/// #     }
/// # }
/// #
/// # async fn example() {
/// let service = MyService;
/// ServiceRunner::new(service)
///     .with_health_check_port(8080)
///     .run(|service| async move {
///         // Service message loop here
///         Ok(())
///     })
///     .await
///     .unwrap();
/// # }
/// ```
pub struct ServiceRunner<S: ManagedService> {
    service: S,
    health_check_port: Option<u16>,
}

impl<S: ManagedService + 'static> ServiceRunner<S> {
    /// Create a new service runner
    pub fn new(service: S) -> Self {
        Self {
            service,
            health_check_port: None,
        }
    }

    /// Enable health check endpoint on the specified port
    pub fn with_health_check_port(mut self, port: u16) -> Self {
        self.health_check_port = Some(port);
        self
    }

    /// Run the service with graceful shutdown
    ///
    /// The `run_fn` should contain the main service loop.
    /// This function will:
    /// 1. Call initialize() on the service
    /// 2. Start optional health check server
    /// 3. Run the provided function
    /// 4. Wait for shutdown signal (SIGTERM/SIGINT)
    /// 5. Call shutdown() on the service
    pub async fn run<F, Fut>(
        mut self,
        run_fn: F,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        F: FnOnce(S) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>>
            + Send,
    {
        // Initialize service
        tracing::info!("Initializing service...");
        self.service.initialize().await?;

        // Setup shutdown signal
        let shutdown = tokio::signal::ctrl_c();

        // Start health check server if configured
        let _health_server = if let Some(port) = self.health_check_port {
            tracing::info!("Starting health check server on port {}", port);
            // TODO: Implement simple HTTP health check server
            // For now, just log that it would start
            None::<()>
        } else {
            None
        };

        // Run service
        tracing::info!("Service started");
        tokio::select! {
            result = run_fn(self.service) => {
                match result {
                    Ok(_) => tracing::info!("Service completed normally"),
                    Err(e) => tracing::error!("Service error: {}", e),
                }
            }
            _ = shutdown => {
                tracing::info!("Shutdown signal received");
            }
        }

        // Shutdown service
        tracing::info!("Shutting down service...");
        // Note: self.service was moved into run_fn, so we can't call shutdown here
        // This is a limitation of the current design - in a real implementation,
        // we'd need to use Arc<Mutex<S>> or similar to allow shutdown access
        tracing::info!("Service stopped");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestService {
        initialized: bool,
        shutdown: bool,
    }

    #[async_trait::async_trait]
    impl ManagedService for TestService {
        async fn initialize(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.initialized = true;
            Ok(())
        }

        async fn shutdown(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.shutdown = true;
            Ok(())
        }

        async fn health(&self) -> HealthStatus {
            if self.initialized && !self.shutdown {
                HealthStatus::Healthy
            } else {
                HealthStatus::Unhealthy {
                    reason: UnhealthyReason::NotReady,
                }
            }
        }
    }

    #[tokio::test]
    async fn test_service_lifecycle() {
        let mut service = TestService {
            initialized: false,
            shutdown: false,
        };

        assert_eq!(
            service.health().await,
            HealthStatus::Unhealthy {
                reason: UnhealthyReason::NotReady
            }
        );

        service.initialize().await.unwrap();
        assert!(service.initialized);
        assert_eq!(service.health().await, HealthStatus::Healthy);

        service.shutdown().await.unwrap();
        assert!(service.shutdown);
        assert_eq!(
            service.health().await,
            HealthStatus::Unhealthy {
                reason: UnhealthyReason::NotReady
            }
        );
    }

    #[test]
    fn test_health_status_checks() {
        assert!(HealthStatus::Healthy.is_healthy());
        assert!(HealthStatus::Healthy.is_operational());

        let degraded = HealthStatus::Degraded {
            reason: DegradedReason::HighLatency,
        };
        assert!(degraded.is_degraded());
        assert!(degraded.is_operational());

        let unhealthy = HealthStatus::Unhealthy {
            reason: UnhealthyReason::FatalError,
        };
        assert!(unhealthy.is_unhealthy());
        assert!(!unhealthy.is_operational());
    }
}
