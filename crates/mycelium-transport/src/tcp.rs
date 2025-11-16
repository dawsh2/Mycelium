use crate::buffer_pool::BufferPool;
use crate::error::Result;
use crate::stream::StreamSubscriber;
use crate::stream_transport::{spawn_client_reader, StreamPublisher, StreamTransportState};
use mycelium_protocol::Message;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

const DEFAULT_CHANNEL_CAPACITY: usize = 1000;

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
    write_half: Option<Arc<Mutex<OwnedWriteHalf>>>,
    state: StreamTransportState,
}

impl TcpTransport {
    /// Create a new TCP transport (server side)
    ///
    /// Binds to the address and starts accepting connections
    pub async fn bind(addr: SocketAddr) -> Result<Self> {
        Self::bind_with_buffer_pool(addr, None).await
    }

    /// Create a new TCP transport with buffer pool (server side)
    ///
    /// Binds to the address and starts accepting connections.
    /// Uses buffer pool for zero-allocation reading (recommended for high throughput).
    pub async fn bind_with_buffer_pool(
        addr: SocketAddr,
        buffer_pool: Option<BufferPool>,
    ) -> Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        let bind_addr = listener.local_addr()?;
        let state = StreamTransportState::new(DEFAULT_CHANNEL_CAPACITY, buffer_pool);

        let transport = Self {
            bind_addr,
            listener: Some(Arc::new(listener)),
            write_half: None,
            state: state.clone(),
        };

        transport.spawn_accept_loop();
        Ok(transport)
    }

    /// Connect to a TCP transport (client side)
    ///
    /// Establishes a persistent connection that will be reused for all publishes.
    pub async fn connect(addr: SocketAddr) -> Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        let (reader, writer) = stream.into_split();
        let state = StreamTransportState::new(DEFAULT_CHANNEL_CAPACITY, None);

        spawn_client_reader(reader, state.clone(), "TCP");

        Ok(Self {
            bind_addr: addr,
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
                    Ok((stream, addr)) => {
                        tracing::debug!("Accepted TCP connection from {}", addr);
                        spawn_client_reader(stream, state.clone(), "TCP");
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
    pub fn publisher<M: Message>(&self) -> Option<TcpPublisher<M>> {
        let write_half = self.write_half.as_ref()?.clone();
        Some(StreamPublisher::new(write_half))
    }

    /// Create a subscriber for a message type
    ///
    /// Registers the type_id → topic mapping for efficient routing.
    pub fn subscriber<M: Message>(&self) -> TcpSubscriber<M> {
        self.state.register_type::<M>();

        let tx = self.state.get_or_create_channel(M::TOPIC);
        let rx = tx.subscribe();
        TcpSubscriber::new(rx)
    }

    /// Get the bound address
    pub fn local_addr(&self) -> SocketAddr {
        self.bind_addr
    }
}

/// Publisher for TCP socket transport with persistent connection
pub type TcpPublisher<M> = StreamPublisher<M, OwnedWriteHalf>;

/// Subscriber for TCP socket transport (re-exported from stream module)
pub type TcpSubscriber<M> = StreamSubscriber<M>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::write_message;
    use mycelium_protocol::impl_message;
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;
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

        assert_ne!(transport.local_addr().port(), 0);
    }

    #[tokio::test]
    async fn test_tcp_publisher() {
        let addr = "127.0.0.1:0".parse().unwrap();
        let server = TcpTransport::bind(addr).await.unwrap();
        let server_addr = server.local_addr();

        let mut sub = server.subscriber::<TestMsg>();

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let client = TcpTransport::connect(server_addr).await.unwrap();
        let pub_ = client.publisher::<TestMsg>().unwrap();

        let msg = TestMsg { value: 42 };
        pub_.publish(msg.clone()).await.unwrap();

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

        let client = TcpTransport::connect(server_addr).await.unwrap();
        let pub_ = client.publisher::<TestMsg>().unwrap();

        for i in 0..3 {
            pub_.publish(TestMsg { value: i }).await.unwrap();
        }

        for i in 0..3 {
            let received = tokio::time::timeout(tokio::time::Duration::from_secs(1), sub.recv())
                .await
                .expect("Timeout waiting for message");

            assert_eq!(received.unwrap().value, i);
        }
    }

    #[tokio::test]
    async fn test_tcp_connect_subscriber_receives() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let (ready_tx, ready_rx) = oneshot::channel();
        let server_handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            ready_rx.await.unwrap();
            let msg = TestMsg { value: 123 };
            write_message(&mut stream, &msg).await.unwrap();
        });

        let transport = TcpTransport::connect(addr).await.unwrap();
        let mut sub = transport.subscriber::<TestMsg>();

        ready_tx.send(()).unwrap();

        let received = tokio::time::timeout(tokio::time::Duration::from_secs(1), sub.recv())
            .await
            .expect("Timeout waiting for TCP message")
            .expect("TCP stream closed unexpectedly");

        assert_eq!(received.value, 123);

        server_handle.await.unwrap();
    }
}
