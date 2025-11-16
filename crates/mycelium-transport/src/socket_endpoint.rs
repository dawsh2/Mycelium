use crate::bridge::{BridgeFanout, BridgeFrame};
use crate::codec::read_frame;
use crate::error::{Result, TransportError};
use crate::local::LocalTransport;
use crate::service_metrics::ServiceMetrics;
use mycelium_protocol::codec::HEADER_SIZE;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UnixListener, UnixStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// Handle that controls the lifetime of a socket endpoint bridge.
///
/// Calling `shutdown()` gracefully stops the listener and all active
/// connection pumps. Dropping the handle triggers the same shutdown
/// but does not await completion.
pub struct SocketEndpointHandle {
    shutdown: watch::Sender<bool>,
    join: Option<JoinHandle<()>>,
}

impl SocketEndpointHandle {
    pub async fn shutdown(mut self) -> Result<()> {
        let _ = self.shutdown.send(true);
        if let Some(join) = self.join.take() {
            join.await.map_err(|err| {
                TransportError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("socket bridge task failed: {err}"),
                ))
            })?;
        }
        Ok(())
    }
}

impl Drop for SocketEndpointHandle {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        if let Some(join) = self.join.take() {
            join.abort();
        }
    }
}

pub(crate) async fn bind_unix_endpoint(
    socket_path: PathBuf,
    local: LocalTransport,
    schema_digest: Option<[u8; 32]>,
    stats: Option<Arc<EndpointStats>>,
) -> Result<SocketEndpointHandle> {
    if let Some(parent) = socket_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let _ = tokio::fs::remove_file(&socket_path).await;

    let listener = UnixListener::bind(&socket_path)?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let bridge = BridgeContext::new(local, schema_digest, stats);

    let join = tokio::spawn(run_unix_listener(
        listener,
        socket_path.clone(),
        bridge,
        shutdown_rx,
    ));

    Ok(SocketEndpointHandle {
        shutdown: shutdown_tx,
        join: Some(join),
    })
}

pub(crate) async fn bind_tcp_endpoint(
    addr: SocketAddr,
    local: LocalTransport,
    schema_digest: Option<[u8; 32]>,
    stats: Option<Arc<EndpointStats>>,
) -> Result<(SocketEndpointHandle, SocketAddr)> {
    let listener = TcpListener::bind(addr).await?;
    let listen_addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let bridge = BridgeContext::new(local, schema_digest, stats);

    let join = tokio::spawn(run_tcp_listener(listener, bridge, shutdown_rx));

    Ok((
        SocketEndpointHandle {
            shutdown: shutdown_tx,
            join: Some(join),
        },
        listen_addr,
    ))
}

async fn run_unix_listener(
    listener: UnixListener,
    socket_path: PathBuf,
    bridge: BridgeContext,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            accept_res = listener.accept() => match accept_res {
                Ok((mut stream, _addr)) => {
                    if let Err(err) = bridge.verify_handshake(&mut stream).await {
                        tracing::warn!("Unix bridge handshake failed: {}", err);
                        continue;
                    }
                    spawn_stream_pump_unix(stream, bridge.clone(), shutdown.clone());
                }
                Err(err) => {
                    tracing::error!("Unix endpoint accept error: {}", err);
                    break;
                }
            }
        }
    }

    let _ = tokio::fs::remove_file(&socket_path).await;
}

async fn run_tcp_listener(
    listener: TcpListener,
    bridge: BridgeContext,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            accept_res = listener.accept() => match accept_res {
                Ok((mut stream, addr)) => {
                    tracing::debug!("Accepted TCP bridge client: {}", addr);
                    if let Err(err) = bridge.verify_handshake(&mut stream).await {
                        tracing::warn!("TCP bridge handshake failed: {}", err);
                        continue;
                    }
                    spawn_stream_pump_tcp(stream, bridge.clone(), shutdown.clone());
                }
                Err(err) => {
                    tracing::error!("TCP endpoint accept error: {}", err);
                    break;
                }
            }
        }
    }
}

fn spawn_stream_pump_unix(
    stream: UnixStream,
    bridge: BridgeContext,
    shutdown: watch::Receiver<bool>,
) {
    let (reader, writer) = stream.into_split();
    spawn_reader(reader, bridge.clone(), shutdown.clone(), "unix");
    spawn_writer(writer, bridge.fanout.clone(), shutdown, "unix");
}

fn spawn_stream_pump_tcp(
    stream: TcpStream,
    bridge: BridgeContext,
    shutdown: watch::Receiver<bool>,
) {
    let (reader, writer) = stream.into_split();
    spawn_reader(reader, bridge.clone(), shutdown.clone(), "tcp");
    spawn_writer(writer, bridge.fanout.clone(), shutdown, "tcp");
}

fn spawn_writer<W>(
    mut writer: W,
    fanout: BridgeFanout,
    mut shutdown: watch::Receiver<bool>,
    label: &'static str,
) where
    W: AsyncWriteExt + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut rx = fanout.subscribe();

        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                frame = rx.recv() => match frame {
                    Ok(frame) => {
                        if let Err(err) = writer.write_all(&frame.bytes).await {
                            tracing::debug!("{} bridge client disconnected: {}", label, err);
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!("{} bridge client lagged by {} frames", label, skipped);
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    });
}

fn spawn_reader<R>(
    mut reader: R,
    bridge: BridgeContext,
    mut shutdown: watch::Receiver<bool>,
    label: &'static str,
) where
    R: AsyncReadExt + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                frame = read_frame(&mut reader) => match frame {
                    Ok((type_id, bytes)) => {
                        bridge.handle_incoming(type_id, bytes).await;
                    }
                    Err(e) => {
                        tracing::debug!("{} bridge reader closed: {}", label, e);
                        break;
                    }
                }
            }
        }
    });
}

#[derive(Clone)]
struct BridgeContext {
    local: LocalTransport,
    fanout: BridgeFanout,
    schema_digest: Option<[u8; 32]>,
    stats: Arc<EndpointStats>,
}

impl BridgeContext {
    fn new(
        local: LocalTransport,
        schema_digest: Option<[u8; 32]>,
        stats: Option<Arc<EndpointStats>>,
    ) -> Self {
        let fanout = local.bridge_fanout();
        Self {
            local,
            fanout,
            schema_digest,
            stats: stats.unwrap_or_else(|| Arc::new(EndpointStats::default())),
        }
    }

    async fn handle_incoming(&self, type_id: u16, bytes: Vec<u8>) {
        if bytes.len() < HEADER_SIZE {
            tracing::warn!("Bridge received frame shorter than TLV header");
            return;
        }

        let payload = &bytes[HEADER_SIZE..];
        match self.local.build_envelope_from_payload(type_id, payload) {
            Ok(envelope) => {
                if let Err(err) = self.local.dispatch_envelope(envelope.clone()) {
                    if !matches!(err, TransportError::NoSubscribers { .. }) {
                        tracing::warn!("Bridge dispatch error: {}", err);
                    }
                }

                let frame = BridgeFrame {
                    type_id,
                    topic: envelope.topic.clone(),
                    bytes: Arc::new(bytes),
                };
                self.fanout.send(frame);
                self.stats.record_forwarded();
            }
            Err(err) => {
                tracing::warn!(
                    "Failed to decode incoming frame type_id {}: {}",
                    type_id,
                    err
                );
            }
        }
    }

    async fn verify_handshake<S>(&self, stream: &mut S) -> Result<()>
    where
        S: AsyncReadExt + Unpin,
    {
        let Some(expected) = self.schema_digest else {
            return Ok(());
        };

        let mut len_buf = [0u8; 2];
        stream.read_exact(&mut len_buf).await?;
        let len = u16::from_le_bytes(len_buf) as usize;
        if len != expected.len() {
            return Err(TransportError::HandshakeFailed(format!(
                "expected digest length {}, got {}",
                expected.len(),
                len
            )));
        }

        let mut received = vec![0u8; len];
        stream.read_exact(&mut received).await?;
        if received.as_slice() != expected {
            self.stats.record_handshake_failure();
            return Err(TransportError::HandshakeFailed(
                "schema digest mismatch".to_string(),
            ));
        }
        self.stats.record_connection();
        Ok(())
    }
}

pub(crate) struct EndpointStats {
    connections: AtomicU64,
    handshake_failures: AtomicU64,
    frames_forwarded: AtomicU64,
    metrics: OnceLock<ServiceMetrics>,
}

impl EndpointStats {
    pub(crate) fn attach_metrics(&self, metrics: ServiceMetrics) {
        let _ = self.metrics.set(metrics);
    }

    fn record_connection(&self) {
        self.connections.fetch_add(1, Ordering::Relaxed);
        if let Some(metrics) = self.metrics.get() {
            metrics.incr_counter(METRIC_CONNECTIONS, 1);
        }
    }

    fn record_handshake_failure(&self) {
        self.handshake_failures.fetch_add(1, Ordering::Relaxed);
        if let Some(metrics) = self.metrics.get() {
            metrics.incr_counter(METRIC_HANDSHAKE_FAILURES, 1);
        }
    }

    fn record_forwarded(&self) {
        self.frames_forwarded.fetch_add(1, Ordering::Relaxed);
        if let Some(metrics) = self.metrics.get() {
            metrics.incr_counter(METRIC_FRAMES_FORWARDED, 1);
        }
    }

    pub(crate) fn snapshot(&self) -> (u64, u64, u64) {
        (
            self.connections.load(Ordering::Relaxed),
            self.handshake_failures.load(Ordering::Relaxed),
            self.frames_forwarded.load(Ordering::Relaxed),
        )
    }
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

const METRIC_CONNECTIONS: &str = "socket_endpoint.connections_total";
const METRIC_HANDSHAKE_FAILURES: &str = "socket_endpoint.handshake_failures_total";
const METRIC_FRAMES_FORWARDED: &str = "socket_endpoint.frames_forwarded_total";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_stats_update_metrics() {
        let stats = EndpointStats::default();
        let metrics = ServiceMetrics::new();
        stats.attach_metrics(metrics.clone());

        stats.record_connection();
        stats.record_handshake_failure();
        stats.record_forwarded();

        assert_eq!(metrics.counter(METRIC_CONNECTIONS), Some(1));
        assert_eq!(metrics.counter(METRIC_HANDSHAKE_FAILURES), Some(1));
        assert_eq!(metrics.counter(METRIC_FRAMES_FORWARDED), Some(1));
    }
}
