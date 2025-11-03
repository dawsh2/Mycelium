use crate::codec::write_message;
use crate::error::Result;
use crate::stream::{handle_stream_connection, StreamSubscriber};
use dashmap::DashMap;
use mycelium_protocol::{Envelope, Message};
use std::marker::PhantomData;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, Mutex, oneshot};

/// TCP socket transport for distributed inter-process communication
///
/// Uses TCP sockets with TLV framing for message passing
/// between services across different machines.
///
/// Performance: ~10-50μs per message (serialization + network overhead)
pub struct TcpTransport {
    bind_addr: SocketAddr,
    listener: Option<Arc<TcpListener>>,
    /// Client-side persistent connection (for publishers)
    connection: Option<Arc<Mutex<TcpStream>>>,
    /// Topic → broadcast channel mapping
    channels: Arc<DashMap<String, broadcast::Sender<Envelope>>>,
    /// Type ID → Topic mapping (for efficient routing)
    type_to_topic: Arc<DashMap<u16, String>>,
    channel_capacity: usize,
}

impl TcpTransport {
    /// Create a new TCP transport (server side)
    ///
    /// Binds to the address and starts accepting connections
    pub async fn bind(addr: SocketAddr) -> Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        let bind_addr = listener.local_addr()?;

        let transport = Self {
            bind_addr,
            listener: Some(Arc::new(listener)),
            connection: None,
            channels: Arc::new(DashMap::new()),
            type_to_topic: Arc::new(DashMap::new()),
            channel_capacity: 1000,
        };

        // Spawn accept loop
        transport.spawn_accept_loop();

        Ok(transport)
    }

    /// Connect to a TCP transport (client side)
    ///
    /// Establishes a persistent connection that will be reused for all publishes.
    pub async fn connect(addr: SocketAddr) -> Result<Self> {
        let stream = TcpStream::connect(addr).await?;

        Ok(Self {
            bind_addr: addr,
            listener: None,
            connection: Some(Arc::new(Mutex::new(stream))),
            channels: Arc::new(DashMap::new()),
            type_to_topic: Arc::new(DashMap::new()),
            channel_capacity: 1000,
        })
    }

    /// Spawn the accept loop for incoming connections
    fn spawn_accept_loop(&self) {
        let Some(listener) = self.listener.clone() else {
            return;
        };

        let channels = Arc::clone(&self.channels);
        let type_to_topic = Arc::clone(&self.type_to_topic);

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        tracing::debug!("Accepted TCP connection from {}", addr);
                        let channels = Arc::clone(&channels);
                        let type_to_topic = Arc::clone(&type_to_topic);
                        tokio::spawn(async move {
                            if let Err(e) = handle_stream_connection(stream, channels, type_to_topic).await {
                                tracing::error!("Connection error from {}: {}", addr, e);
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
    ///
    /// Returns None if this is a server-side transport (no client connection).
    pub fn publisher<M: Message>(&self) -> Option<TcpPublisher<M>> {
        let connection = self.connection.as_ref()?.clone();
        Some(TcpPublisher {
            connection,
            _phantom: PhantomData,
        })
    }

    /// Create a subscriber for a message type
    ///
    /// Registers the type_id → topic mapping for efficient routing.
    pub fn subscriber<M: Message>(&self) -> TcpSubscriber<M> {
        // Register type_id → topic mapping
        self.type_to_topic.insert(M::TYPE_ID, M::TOPIC.to_string());

        let tx = self.get_or_create_channel(M::TOPIC);
        let rx = tx.subscribe();
        TcpSubscriber::new(rx)
    }

    /// Get the bound address
    pub fn local_addr(&self) -> SocketAddr {
        self.bind_addr
    }
}

/// Publisher for TCP socket transport with persistent connection
pub struct TcpPublisher<M> {
    connection: Arc<Mutex<TcpStream>>,
    _phantom: PhantomData<M>,
}

impl<M: Message> TcpPublisher<M>
where
    M: zerocopy::AsBytes,
{
    /// Publish a message over TCP socket using the persistent connection
    ///
    /// The connection is shared across all publishers from the same transport,
    /// protected by a mutex. This eliminates the overhead of creating a new
    /// connection for each message.
    pub async fn publish(&self, msg: M) -> Result<()> {
        let mut stream = self.connection.lock().await;

        // Write message to persistent connection
        write_message(&mut *stream, &msg).await?;

        // Flush to ensure message is sent
        stream.flush().await?;

        Ok(())
    }
}

/// Subscriber for TCP socket transport (re-exported from stream module)
pub type TcpSubscriber<M> = StreamSubscriber<M>;

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

    impl_message!(TestMsg, 88, "test-tcp");

    #[tokio::test]
    async fn test_tcp_transport_bind() {
        let addr = "127.0.0.1:0".parse().unwrap();
        let transport = TcpTransport::bind(addr).await.unwrap();

        // Should have bound to a random port
        assert_ne!(transport.local_addr().port(), 0);
    }

    #[tokio::test]
    async fn test_tcp_publisher() {
        let addr = "127.0.0.1:0".parse().unwrap();
        let server = TcpTransport::bind(addr).await.unwrap();
        let server_addr = server.local_addr();

        let mut sub = server.subscriber::<TestMsg>();

        // Give server time to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Client publishes
        let client = TcpTransport::connect(server_addr).await.unwrap();
        let pub_ = client.publisher::<TestMsg>().unwrap();

        let msg = TestMsg { value: 42 };
        pub_.publish(msg.clone()).await.unwrap();

        // Wait for message with timeout
        let received = tokio::time::timeout(tokio::time::Duration::from_secs(1), sub.recv())
            .await
            .expect("Timeout waiting for message");

        assert_eq!(received.unwrap().value, 42);
    }

    #[tokio::test]
    async fn test_tcp_multiple_messages() {
        let addr = "127.0.0.1:0".parse().unwrap();
        let server = TcpTransport::bind(addr).await.unwrap();
        let server_addr = server.local_addr();

        let mut sub = server.subscriber::<TestMsg>();

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Client publishes multiple messages
        let client = TcpTransport::connect(server_addr).await.unwrap();
        let pub_ = client.publisher::<TestMsg>().unwrap();

        for i in 0..3 {
            pub_.publish(TestMsg { value: i }).await.unwrap();
        }

        // Receive messages
        for i in 0..3 {
            let received = tokio::time::timeout(tokio::time::Duration::from_secs(1), sub.recv())
                .await
                .expect("Timeout waiting for message");

            assert_eq!(received.unwrap().value, i);
        }
    }
}
