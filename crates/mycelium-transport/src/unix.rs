use crate::codec::write_message;
use crate::error::{Result, TransportError};
use crate::stream::{handle_stream_connection, StreamSubscriber};
use dashmap::DashMap;
use mycelium_protocol::{Envelope, Message};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;

/// Unix socket transport for inter-process communication
///
/// Uses Unix domain sockets with TLV framing for message passing
/// between services in different processes on the same machine.
///
/// Performance: ~2-5μs per message (serialization + socket overhead)
pub struct UnixTransport {
    socket_path: PathBuf,
    listener: Option<Arc<UnixListener>>,
    // Topic -> broadcast channel
    channels: Arc<DashMap<String, broadcast::Sender<Envelope>>>,
    channel_capacity: usize,
}

impl UnixTransport {
    /// Create a new Unix socket transport (server side)
    ///
    /// Binds to the socket path and starts accepting connections
    pub async fn bind<P: AsRef<Path>>(socket_path: P) -> Result<Self> {
        let socket_path = socket_path.as_ref().to_path_buf();

        // Create parent directory if needed
        if let Some(parent) = socket_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Remove old socket if it exists
        let _ = tokio::fs::remove_file(&socket_path).await;

        // Bind listener
        let listener = UnixListener::bind(&socket_path)?;

        let transport = Self {
            socket_path,
            listener: Some(Arc::new(listener)),
            channels: Arc::new(DashMap::new()),
            channel_capacity: 1000,
        };

        // Spawn accept loop
        transport.spawn_accept_loop();

        Ok(transport)
    }

    /// Connect to a Unix socket transport (client side)
    pub async fn connect<P: AsRef<Path>>(socket_path: P) -> Result<Self> {
        let socket_path = socket_path.as_ref().to_path_buf();

        // Verify socket exists
        if !socket_path.exists() {
            return Err(TransportError::ServiceNotFound(
                socket_path.display().to_string(),
            ));
        }

        Ok(Self {
            socket_path,
            listener: None,
            channels: Arc::new(DashMap::new()),
            channel_capacity: 1000,
        })
    }

    /// Spawn the accept loop for incoming connections
    fn spawn_accept_loop(&self) {
        let Some(listener) = self.listener.clone() else {
            return;
        };

        let channels = Arc::clone(&self.channels);

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _addr)) => {
                        tracing::debug!("Accepted Unix socket connection");
                        let channels = Arc::clone(&channels);
                        tokio::spawn(async move {
                            if let Err(e) = handle_stream_connection(stream, channels).await {
                                tracing::error!("Connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        tracing::error!("Accept error: {}", e);
                    }
                }
            }
        });
    }

    /// Get or create a broadcast channel for a topic
    fn get_or_create_channel(&self, topic: &str) -> broadcast::Sender<Envelope> {
        self.channels
            .entry(topic.to_string())
            .or_insert_with(|| broadcast::channel(self.channel_capacity).0)
            .clone()
    }

    /// Create a publisher for a message type
    pub fn publisher<M: Message>(&self) -> UnixPublisher<M> {
        UnixPublisher {
            socket_path: self.socket_path.clone(),
            _phantom: PhantomData,
        }
    }

    /// Create a subscriber for a message type
    pub fn subscriber<M: Message>(&self) -> UnixSubscriber<M> {
        let tx = self.get_or_create_channel(M::TOPIC);
        let rx = tx.subscribe();
        UnixSubscriber::new(rx)
    }
}

/// Publisher for Unix socket transport
pub struct UnixPublisher<M> {
    socket_path: PathBuf,
    _phantom: PhantomData<M>,
}

impl<M: Message> UnixPublisher<M>
where
    M: rkyv::Serialize<rkyv::ser::serializers::AllocSerializer<1024>>,
{
    /// Publish a message over Unix socket
    pub async fn publish(&self, msg: M) -> Result<()> {
        // Connect to server
        let mut stream = UnixStream::connect(&self.socket_path).await?;

        // Write message
        write_message(&mut stream, &msg).await?;

        // Graceful shutdown
        stream.shutdown().await?;

        Ok(())
    }
}

/// Subscriber for Unix socket transport (re-exported from stream module)
pub type UnixSubscriber<M> = StreamSubscriber<M>;

#[cfg(test)]
mod tests {
    use super::*;
    use mycelium_protocol::impl_message;
    use rkyv::{Archive, Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Archive, Serialize, Deserialize)]
    #[archive(check_bytes)]
    struct TestMsg {
        value: u64,
    }

    impl_message!(TestMsg, 99, "test-unix");

    #[tokio::test]
    async fn test_unix_transport_bind() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        let _transport = UnixTransport::bind(&socket_path).await.unwrap();
        assert!(socket_path.exists());
    }

    #[tokio::test]
    async fn test_unix_transport_connect() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        // Bind server
        let _server = UnixTransport::bind(&socket_path).await.unwrap();

        // Connect client
        let _client = UnixTransport::connect(&socket_path).await.unwrap();
    }

    #[tokio::test]
    async fn test_unix_publisher() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        // Bind server
        let server = UnixTransport::bind(&socket_path).await.unwrap();
        let mut sub = server.subscriber::<TestMsg>();

        // Give server time to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Client publishes
        let client = UnixTransport::connect(&socket_path).await.unwrap();
        let pub_ = client.publisher::<TestMsg>();

        let msg = TestMsg { value: 42 };
        pub_.publish(msg.clone()).await.unwrap();

        // Wait for message with timeout
        let received = tokio::time::timeout(tokio::time::Duration::from_secs(1), sub.recv())
            .await
            .expect("Timeout waiting for message");

        assert_eq!(received.unwrap().value, 42);
    }
}
