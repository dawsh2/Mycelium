//! ServiceRuntime - Spawns and manages services with supervision

use crate::bus::MessageBus;
use crate::service_context::ServiceContext;
use crate::service_metrics::ServiceMetrics;
use anyhow::anyhow;
use tokio::sync::{broadcast, oneshot, Mutex};

/// ServiceRuntime manages service lifecycle and supervision
pub struct ServiceRuntime {
    bus: std::sync::Arc<MessageBus>,
    shutdown_tx: broadcast::Sender<()>,
    handles: Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl ServiceRuntime {
    /// Create a new ServiceRuntime with a MessageBus
    pub fn new(bus: MessageBus) -> Self {
        let (shutdown_tx, _) = broadcast::channel(16);

        Self {
            bus: std::sync::Arc::new(bus),
            shutdown_tx,
            handles: Mutex::new(Vec::new()),
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
        let (completion_tx, completion_rx) = oneshot::channel();

        let handle = tokio::spawn(async move {
            let result = run_supervised(service, ctx, metrics, service_name).await;
            let _ = completion_tx.send(result);
        });

        self.handles.lock().await.push(handle);

        Ok(ServiceHandle {
            completion: completion_rx,
        })
    }

    /// Gracefully shutdown all services
    pub async fn shutdown(&self) -> anyhow::Result<()> {
        tracing::info!("Shutting down service runtime");
        let _ = self.shutdown_tx.send(());

        let mut handles = self.handles.lock().await;
        while let Some(handle) = handles.pop() {
            if let Err(join_err) = handle.await {
                tracing::error!("Service task panicked: {join_err}");
            }
        }
        Ok(())
    }
}

/// Handle to a running service
pub struct ServiceHandle {
    completion: oneshot::Receiver<anyhow::Result<()>>,
}

impl ServiceHandle {
    /// Wait for the service to complete
    pub async fn join(self) -> anyhow::Result<()> {
        match self.completion.await {
            Ok(result) => result,
            Err(_) => Err(anyhow!("service task dropped before completion")),
        }
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
) -> anyhow::Result<()>
where
    S: Service,
{
    let mut retry_count = 0;
    let max_retries = 5;

    service.startup().await.map_err(|e| {
        tracing::error!(service = S::NAME, error = %e, "Service startup failed");
        metrics.record_error();
        e
    })?;

    let mut result = loop {
        match service.run(&ctx).await {
            Ok(()) => {
                tracing::info!(service = S::NAME, "Service exited cleanly");
                break Ok(());
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
                    break Err(e);
                }

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
    };

    if let Err(e) = service.shutdown().await {
        tracing::error!(service = S::NAME, error = %e, "Service shutdown failed");
        metrics.record_error();
        if result.is_ok() {
            result = Err(e);
        }
    }

    metrics.print_summary(&service_name);
    result
}

fn generate_trace_id() -> String {
    use std::time::SystemTime;

    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    format!("{:016x}", nanos)
}
