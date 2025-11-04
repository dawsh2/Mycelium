//! ServiceRuntime - Spawns and manages services with supervision

use crate::context::ServiceContext;
use crate::metrics::ServiceMetrics;
use mycelium_transport::MessageBus;
use tokio::sync::broadcast;

/// ServiceRuntime manages service lifecycle and supervision
pub struct ServiceRuntime {
    bus: std::sync::Arc<MessageBus>,
    shutdown_tx: broadcast::Sender<()>,
}

impl ServiceRuntime {
    /// Create a new ServiceRuntime with a MessageBus
    pub fn new(bus: MessageBus) -> Self {
        let (shutdown_tx, _) = broadcast::channel(16);

        Self {
            bus: std::sync::Arc::new(bus),
            shutdown_tx,
        }
    }

    /// Spawn a service with supervision
    pub async fn spawn_service<S>(&self, service: S) -> anyhow::Result<ServiceHandle>
    where
        S: Service + Send + 'static,
    {
        let trace_id = generate_trace_id();
        let span = tracing::info_span!(
            "service",
            service = S::NAME,
            trace_id = %trace_id,
        );

        let metrics = ServiceMetrics::new();

        let ctx = ServiceContext::new(
            std::sync::Arc::clone(&self.bus),
            trace_id,
            span.clone(),
            self.shutdown_tx.subscribe(),
            metrics.clone(),
        );

        let service_name = S::NAME.to_string();
        let handle =
            tokio::spawn(async move { run_supervised(service, ctx, metrics, service_name).await });

        Ok(ServiceHandle { handle })
    }

    /// Gracefully shutdown all services
    pub async fn shutdown(&self) -> anyhow::Result<()> {
        tracing::info!("Shutting down service runtime");
        let _ = self.shutdown_tx.send(());
        Ok(())
    }
}

/// Handle to a running service
pub struct ServiceHandle {
    handle: tokio::task::JoinHandle<()>,
}

impl ServiceHandle {
    /// Wait for the service to complete
    pub async fn join(self) -> anyhow::Result<()> {
        self.handle.await?;
        Ok(())
    }
}

/// Trait that all services must implement
pub trait Service {
    /// Service name (for logging/metrics)
    const NAME: &'static str;

    /// Main service loop
    fn run(
        &mut self,
        ctx: &ServiceContext,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;

    /// Optional: Called once before run()
    fn startup(&mut self) -> impl std::future::Future<Output = anyhow::Result<()>> + Send {
        async { Ok(()) }
    }

    /// Optional: Called on graceful shutdown
    fn shutdown(&mut self) -> impl std::future::Future<Output = anyhow::Result<()>> + Send {
        async { Ok(()) }
    }
}

/// Run a service with supervision (exponential backoff retry)
async fn run_supervised<S>(
    mut service: S,
    ctx: ServiceContext,
    metrics: ServiceMetrics,
    service_name: String,
) where
    S: Service,
{
    let mut retry_count = 0;
    let max_retries = 5;

    if let Err(e) = service.startup().await {
        tracing::error!(
            service = S::NAME,
            error = %e,
            "Service startup failed"
        );
        metrics.record_error();
        return;
    }

    loop {
        match service.run(&ctx).await {
            Ok(()) => {
                tracing::info!(service = S::NAME, "Service exited cleanly");
                break;
            }
            Err(e) => {
                tracing::error!(
                    service = S::NAME,
                    error = %e,
                    retry = retry_count,
                    "Service error"
                );

                metrics.record_error();

                retry_count += 1;
                if retry_count >= max_retries {
                    tracing::error!(service = S::NAME, "Max retries exceeded, giving up");
                    break;
                }

                // Record restart
                metrics.record_restart();

                let backoff = std::time::Duration::from_secs(2u64.pow(retry_count));
                tracing::info!(
                    service = S::NAME,
                    backoff_secs = backoff.as_secs(),
                    "Backing off before retry"
                );

                tokio::time::sleep(backoff).await;
            }
        }
    }

    if let Err(e) = service.shutdown().await {
        tracing::error!(
            service = S::NAME,
            error = %e,
            "Service shutdown failed"
        );
        metrics.record_error();
    }

    // Print final metrics summary
    metrics.print_summary(&service_name);
}

fn generate_trace_id() -> String {
    use std::time::SystemTime;

    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    format!("{:016x}", nanos)
}
