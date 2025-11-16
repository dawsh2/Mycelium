use crate::buffer_pool::BufferPool;
use crate::error::{Result, TransportError};
use crate::stream::StreamSubscriber;
use crate::stream_transport::{spawn_client_reader, StreamPublisher, StreamTransportState};
use mycelium_protocol::Message;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::net::unix::OwnedWriteHalf;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;

const DEFAULT_CHANNEL_CAPACITY: usize = 1000;

/// Unix socket transport for inter-process communication
///
/// Uses Unix domain sockets with TLV framing for message passing
/// between services in different processes on the same machine.
///
/// Performance: ~2-5μs per message (serialization + socket overhead)
pub struct UnixTransport {
    /// Socket path (kept for debugging/inspection purposes)
    #[allow(dead_code)]
    socket_path: PathBuf,
    listener: Option<Arc<UnixListener>>,
    /// Client-side persistent connection (for publishers)
    write_half: Option<Arc<Mutex<OwnedWriteHalf>>>,
    state: StreamTransportState,
}

impl UnixTransport {
    /// Create a new Unix socket transport (server side)
    ///
    /// Binds to the socket path and starts accepting connections
    pub async fn bind<P: AsRef<Path>>(socket_path: P) -> Result<Self> {
        Self::bind_with_buffer_pool(socket_path, None).await
    }

    /// Create a new Unix socket transport with buffer pool (server side)
    ///
    /// Binds to the socket path and starts accepting connections.
    /// Uses buffer pool for zero-allocation reading (recommended for high throughput).
    pub async fn bind_with_buffer_pool<P: AsRef<Path>>(
        socket_path: P,
        buffer_pool: Option<BufferPool>,
    ) -> Result<Self> {
        let socket_path = socket_path.as_ref().to_path_buf();

        if let Some(parent) = socket_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let _ = tokio::fs::remove_file(&socket_path).await;

        let listener = UnixListener::bind(&socket_path)?;
        let state = StreamTransportState::new(DEFAULT_CHANNEL_CAPACITY, buffer_pool);

        let transport = Self {
            socket_path,
            listener: Some(Arc::new(listener)),
            write_half: None,
            state: state.clone(),
        };

        transport.spawn_accept_loop();
        Ok(transport)
    }

    /// Connect to a Unix socket transport (client side)
    ///
    /// Establishes a persistent connection that will be reused for all publishes.
    pub async fn connect<P: AsRef<Path>>(socket_path: P) -> Result<Self> {
        let socket_path = socket_path.as_ref().to_path_buf();

        if !socket_path.exists() {
            return Err(TransportError::ServiceNotFound(
                socket_path.display().to_string(),
            ));
        }

        let stream = UnixStream::connect(&socket_path).await?;
        let (reader, writer) = stream.into_split();
        let state = StreamTransportState::new(DEFAULT_CHANNEL_CAPACITY, None);

        spawn_client_reader(reader, state.clone(), "Unix");

        Ok(Self {
            socket_path,
            listener: None,
            write_half: Some(Arc::new(Mutex::new(writer))),
            state,
        })
    }

    fn spawn_accept_loop(&self) {
        let Some(listener) = self.listener.clone() else {
            return;
        };
        let state = self.state.clone();

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _addr)) => {
                        tracing::debug!("Accepted Unix socket connection");
                        spawn_client_reader(stream, state.clone(), "Unix");
                    }
                    Err(e) => {
                        tracing::error!("Accept error: {}", e);
                    }
                }
            }
        });
    }

    /// Create a publisher for a message type
    ///
    /// Returns None if this is a server-side transport (no client connection).
    pub fn publisher<M: Message>(&self) -> Option<UnixPublisher<M>> {
        let write_half = self.write_half.as_ref()?.clone();
        Some(StreamPublisher::new(write_half))
    }

    /// Create a subscriber for a message type
    ///
    /// Registers the type_id → topic mapping for efficient routing.
    pub fn subscriber<M: Message>(&self) -> UnixSubscriber<M> {
        self.state.register_type::<M>();

        let tx = self.state.get_or_create_channel(M::TOPIC);
        let rx = tx.subscribe();
        UnixSubscriber::new(rx)
    }
}

/// Publisher for Unix socket transport with persistent connection
pub type UnixPublisher<M> = StreamPublisher<M, OwnedWriteHalf>;

/// Subscriber for Unix socket transport (re-exported from stream module)
pub type UnixSubscriber<M> = StreamSubscriber<M>;

#[cfg(test)]
mod tests {
    use super::*;
    use mycelium_protocol::impl_message;
    use zerocopy::{AsBytes, FromBytes, FromZeroes};

    #[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
    #[repr(C)]
    struct TestMsg {
        value: u64,
    }

    impl_message!(TestMsg, 99, "test-unix");

    #[tokio::test]
    async fn test_unix_transport_bind() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        let transport = UnixTransport::bind(&socket_path).await.unwrap();
        assert!(transport.listener.is_some());
    }

    #[tokio::test]
    async fn test_unix_transport_connect() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        let _server = UnixTransport::bind(&socket_path).await.unwrap();

        let client = UnixTransport::connect(&socket_path).await.unwrap();
        assert!(client.publisher::<TestMsg>().is_some());
    }

    #[tokio::test]
    async fn test_unix_publisher() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        let server = UnixTransport::bind(&socket_path).await.unwrap();
        let mut sub = server.subscriber::<TestMsg>();

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let client = UnixTransport::connect(&socket_path).await.unwrap();
        let pub_ = client.publisher::<TestMsg>().unwrap();

        let msg = TestMsg { value: 42 };
        pub_.publish(msg.clone()).await.unwrap();

        let received = tokio::time::timeout(tokio::time::Duration::from_secs(1), sub.recv())
            .await
            .expect("Timeout waiting for message");

        assert_eq!(received.unwrap().value, 42);
    }

    #[tokio::test]
    async fn test_unix_subscriber() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        let server = UnixTransport::bind(&socket_path).await.unwrap();
        let mut sub = server.subscriber::<TestMsg>();

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let client = UnixTransport::connect(&socket_path).await.unwrap();
        let pub_ = client.publisher::<TestMsg>().unwrap();

        pub_.publish(TestMsg { value: 55 }).await.unwrap();

        let received = tokio::time::timeout(tokio::time::Duration::from_secs(1), sub.recv())
            .await
            .expect("Timeout waiting for message");

        assert_eq!(received.unwrap().value, 55);
    }
}
