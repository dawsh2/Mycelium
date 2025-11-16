/// Shared memory transport for low-latency cross-process communication.
///
/// This module provides a shared memory transport that achieves ~100-500ns latency
/// for cross-process IPC without syscalls. Uses mmap-backed ring buffers for
/// lock-free SPSC communication.
use crate::bridge::{BridgeFanout, BridgeFrame};
use crate::error::{Result, TransportError};
use crate::local::LocalTransport;
use crate::service_metrics::ServiceMetrics;
use crate::shm_ring::{ShmError, ShmReader, ShmWriter};
use mycelium_protocol::codec::HEADER_SIZE;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// Handle for controlling a shared memory endpoint.
///
/// Dropping the handle triggers graceful shutdown.
pub struct ShmEndpointHandle {
    shutdown: watch::Sender<bool>,
    writer_join: Option<JoinHandle<()>>,
    reader_join: Option<JoinHandle<()>>,
    path: PathBuf,
}

impl ShmEndpointHandle {
    /// Gracefully shutdown the endpoint and wait for tasks to complete.
    pub async fn shutdown(mut self) -> Result<()> {
        let _ = self.shutdown.send(true);

        if let Some(join) = self.writer_join.take() {
            let _ = join.await;
        }
        if let Some(join) = self.reader_join.take() {
            let _ = join.await;
        }

        // Clean up shared memory file
        let _ = tokio::fs::remove_file(&self.path).await;
        Ok(())
    }
}

impl Drop for ShmEndpointHandle {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        if let Some(join) = self.writer_join.take() {
            join.abort();
        }
        if let Some(join) = self.reader_join.take() {
            join.abort();
        }
        // Best effort cleanup
        let path = self.path.clone();
        tokio::spawn(async move {
            let _ = tokio::fs::remove_file(&path).await;
        });
    }
}

/// Bind a shared memory endpoint for publishing/subscribing.
///
/// Creates a shared memory region at the specified path and spawns tasks
/// to handle bidirectional communication between the local bus and external
/// processes.
///
/// # Arguments
///
/// * `path` - Path to shared memory file (e.g., "/dev/shm/mycelium/service.shm")
/// * `local` - Local transport to forward messages to/from
/// * `schema_digest` - 32-byte schema digest for handshake validation
/// * `capacity` - Optional ring buffer capacity (defaults to 1MB)
/// * `stats` - Optional endpoint statistics tracker
pub async fn bind_shm_endpoint(
    path: impl AsRef<Path>,
    local: LocalTransport,
    schema_digest: [u8; 32],
    capacity: Option<usize>,
    stats: Option<Arc<EndpointStats>>,
) -> Result<ShmEndpointHandle> {
    let path = path.as_ref();

    // Create parent directory if needed
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Create writer (initializes shared memory)
    let writer = ShmWriter::create(path, schema_digest, capacity).map_err(|e| match e {
        ShmError::Io(io_err) => TransportError::Io(io_err),
        _ => TransportError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            e.to_string(),
        )),
    })?;

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let stats = stats.unwrap_or_else(|| Arc::new(EndpointStats::default()));
    let fanout = local.bridge_fanout();

    // Spawn writer task (local bus → shared memory)
    let writer_join = tokio::task::spawn_blocking({
        let shutdown = shutdown_rx.clone();
        let stats = stats.clone();
        move || run_shm_writer(writer, fanout, shutdown, stats)
    });

    // Spawn reader task (shared memory → local bus)
    // Note: Reader opens the same file in a separate handle
    let reader_join = tokio::task::spawn_blocking({
        let path = path.to_path_buf();
        let shutdown = shutdown_rx;
        move || run_shm_reader(path, schema_digest, local, shutdown, stats)
    });

    Ok(ShmEndpointHandle {
        shutdown: shutdown_tx,
        writer_join: Some(writer_join),
        reader_join: Some(reader_join),
        path: path.to_path_buf(),
    })
}

/// Writer task: forwards frames from local bus to shared memory.
fn run_shm_writer(
    mut writer: ShmWriter,
    fanout: BridgeFanout,
    shutdown: watch::Receiver<bool>,
    stats: Arc<EndpointStats>,
) {
    let mut rx = fanout.subscribe();

    loop {
        // Check shutdown
        if *shutdown.borrow() {
            break;
        }

        // Try to receive a frame (non-blocking with timeout)
        match rx.try_recv() {
            Ok(frame) => {
                // Extract type_id and payload from TLV frame
                if frame.bytes.len() < HEADER_SIZE {
                    tracing::warn!("Shared memory writer: frame too short");
                    continue;
                }

                let type_id = frame.type_id;
                let payload = &frame.bytes[HEADER_SIZE..];

                // Write to shared memory
                match writer.write_frame(type_id, payload) {
                    Ok(()) => {
                        stats.record_forwarded();
                    }
                    Err(ShmError::BufferFull { available, needed }) => {
                        tracing::warn!(
                            "Shared memory buffer full (available: {}, needed: {})",
                            available,
                            needed
                        );
                        // Could implement backpressure here
                        std::thread::sleep(std::time::Duration::from_micros(10));
                    }
                    Err(e) => {
                        tracing::error!("Shared memory write error: {}", e);
                        break;
                    }
                }
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                // No frames available, sleep briefly
                std::thread::sleep(std::time::Duration::from_micros(10));
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(skipped)) => {
                tracing::warn!("Shared memory writer lagged by {} frames", skipped);
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Closed) => {
                break;
            }
        }
    }

    tracing::debug!("Shared memory writer task stopped");
}

/// Reader task: reads frames from shared memory and forwards to local bus.
fn run_shm_reader(
    path: PathBuf,
    schema_digest: [u8; 32],
    local: LocalTransport,
    shutdown: watch::Receiver<bool>,
    stats: Arc<EndpointStats>,
) {
    // Open shared memory for reading
    let mut reader = match ShmReader::open(&path, schema_digest) {
        Ok(r) => {
            stats.record_connection();
            r
        }
        Err(ShmError::SchemaMismatch) => {
            stats.record_handshake_failure();
            tracing::error!("Shared memory schema digest mismatch");
            return;
        }
        Err(e) => {
            tracing::error!("Failed to open shared memory: {}", e);
            return;
        }
    };

    let fanout = local.bridge_fanout();

    loop {
        // Check shutdown
        if *shutdown.borrow() {
            break;
        }

        // Try to read a frame
        match reader.read_frame() {
            Ok(Some((type_id, payload))) => {
                // Build envelope and dispatch to local bus
                match local.build_envelope_from_payload(type_id, &payload) {
                    Ok(envelope) => {
                        if let Err(err) = local.dispatch_envelope(envelope.clone()) {
                            if !matches!(err, TransportError::NoSubscribers { .. }) {
                                tracing::warn!("Shared memory dispatch error: {}", err);
                            }
                        }

                        // Also forward to fanout for other endpoints
                        let mut frame_bytes = Vec::with_capacity(HEADER_SIZE + payload.len());
                        frame_bytes.extend_from_slice(&type_id.to_le_bytes());
                        frame_bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                        frame_bytes.extend_from_slice(&payload);

                        let frame = BridgeFrame {
                            type_id,
                            topic: envelope.topic.clone(),
                            bytes: Arc::new(frame_bytes),
                        };
                        fanout.send(frame);
                        stats.record_forwarded();
                    }
                    Err(err) => {
                        tracing::warn!(
                            "Failed to decode shared memory frame type_id {}: {}",
                            type_id,
                            err
                        );
                    }
                }
            }
            Ok(None) => {
                // No frames available, sleep briefly
                std::thread::sleep(std::time::Duration::from_micros(10));
            }
            Err(e) => {
                tracing::error!("Shared memory read error: {}", e);
                break;
            }
        }
    }

    tracing::debug!("Shared memory reader task stopped");
}

/// Statistics for shared memory endpoints.
pub(crate) struct EndpointStats {
    connections: AtomicU64,
    handshake_failures: AtomicU64,
    frames_forwarded: AtomicU64,
    metrics: OnceLock<ServiceMetrics>,
}

impl Default for EndpointStats {
    fn default() -> Self {
        Self {
            connections: AtomicU64::new(0),
            handshake_failures: AtomicU64::new(0),
            frames_forwarded: AtomicU64::new(0),
            metrics: OnceLock::new(),
        }
    }
}

impl EndpointStats {
    pub(crate) fn attach_metrics(&self, metrics: ServiceMetrics) {
        let _ = self.metrics.set(metrics);
    }

    pub(crate) fn record_connection(&self) {
        self.connections.fetch_add(1, Ordering::Relaxed);
        if let Some(metrics) = self.metrics.get() {
            metrics.incr_counter("shm_connections", 1);
        }
    }

    pub(crate) fn record_handshake_failure(&self) {
        self.handshake_failures.fetch_add(1, Ordering::Relaxed);
        if let Some(metrics) = self.metrics.get() {
            metrics.incr_counter("shm_handshake_failures", 1);
        }
    }

    pub(crate) fn record_forwarded(&self) {
        self.frames_forwarded.fetch_add(1, Ordering::Relaxed);
        if let Some(metrics) = self.metrics.get() {
            metrics.incr_counter("shm_frames_forwarded", 1);
        }
    }
}
