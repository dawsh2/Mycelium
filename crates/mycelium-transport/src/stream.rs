use crate::codec::{deserialize_message, read_frame};
use crate::error::Result;
use dashmap::DashMap;
use mycelium_protocol::{Envelope, Message};
use std::any::Any;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::broadcast;

/// Raw message frame stored after reading from stream
///
/// Used to share deserialized bytes across multiple subscribers
/// without re-reading from the stream.
#[derive(Clone)]
pub struct RawFrame {
    pub type_id: u16,
    pub bytes: Arc<Vec<u8>>,
}

/// Generic handler for stream-based connections (Unix/TCP)
///
/// Reads TLV frames from the stream and broadcasts them to all registered channels.
/// Each subscriber will filter messages by type_id and deserialize independently.
pub async fn handle_stream_connection<S>(
    mut stream: S,
    channels: Arc<DashMap<String, broadcast::Sender<Envelope>>>,
) -> Result<()>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin + Send,
{
    loop {
        // Read TLV frame from stream
        let (type_id, bytes) = match read_frame(&mut stream).await {
            Ok(frame) => frame,
            Err(e) => {
                tracing::debug!("Connection closed: {}", e);
                return Ok(());
            }
        };

        // Store raw bytes in Arc for zero-copy sharing across subscribers
        let frame = RawFrame {
            type_id,
            bytes: Arc::new(bytes),
        };

        // Broadcast to all channels - subscribers will filter by type_id
        for entry in channels.iter() {
            // Create envelope with RawFrame payload
            // Subscribers will downcast and deserialize on demand
            let envelope = Envelope::from_raw(
                frame.type_id,
                entry.key().clone(),
                Arc::new(frame.clone()) as Arc<dyn Any + Send + Sync>,
            );

            let _ = entry.value().send(envelope);
        }
    }
}

/// Generic subscriber for stream-based transports
///
/// Receives messages over a broadcast channel, filters by type ID,
/// and deserializes from raw frames on demand.
pub struct StreamSubscriber<M> {
    rx: broadcast::Receiver<Envelope>,
    _phantom: PhantomData<M>,
}

impl<M> StreamSubscriber<M> {
    /// Create a new stream subscriber from a broadcast receiver
    pub(crate) fn new(rx: broadcast::Receiver<Envelope>) -> Self {
        Self {
            rx,
            _phantom: PhantomData,
        }
    }
}

impl<M: Message> StreamSubscriber<M> {
    /// Receive the next message
    ///
    /// Filters envelopes by type ID and deserializes matching messages.
    /// Returns None if the channel is closed.
    pub async fn recv(&mut self) -> Option<M> {
        loop {
            // Receive envelope from broadcast channel
            let envelope = match self.rx.recv().await {
                Ok(env) => env,
                Err(broadcast::error::RecvError::Closed) => return None,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("Subscriber lagged by {} messages", n);
                    continue;
                }
            };

            // Filter by type ID
            if envelope.type_id != M::TYPE_ID {
                continue;
            }

            // Downcast payload to RawFrame
            let raw_frame = match envelope.downcast_any::<RawFrame>() {
                Ok(frame) => frame,
                Err(e) => {
                    tracing::error!("Failed to downcast envelope payload to RawFrame: {}", e);
                    continue;
                }
            };

            // Deserialize from raw bytes
            match deserialize_message::<M>(&raw_frame.bytes) {
                Ok(msg) => return Some(msg),
                Err(e) => {
                    tracing::error!("Failed to deserialize message: {}", e);
                    continue;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mycelium_protocol::impl_message;
    use zerocopy::{AsBytes, FromBytes, FromZeroes};
    use tokio::net::{UnixListener, UnixStream};

    #[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
    #[repr(C)]
    struct TestMsg {
        value: u64,
    }

    impl_message!(TestMsg, 123, "test-stream");

    #[tokio::test]
    async fn test_stream_subscriber() {
        let (tx, rx) = broadcast::channel(10);
        let mut sub: StreamSubscriber<TestMsg> = StreamSubscriber::new(rx);

        // Create a raw frame
        let msg = TestMsg { value: 42 };
        let bytes = msg.as_bytes();
        let frame = RawFrame {
            type_id: 123,
            bytes: Arc::new(bytes.to_vec()),
        };

        // Send envelope with raw frame
        let envelope = Envelope::from_raw(
            123,
            "test-stream".to_string(),
            Arc::new(frame) as Arc<dyn Any + Send + Sync>,
        );
        tx.send(envelope).unwrap();

        // Receive and verify
        let msg = sub.recv().await.unwrap();
        assert_eq!(msg.value, 42);
    }

    #[tokio::test]
    async fn test_stream_subscriber_filters_by_type() {
        let (tx, rx) = broadcast::channel(10);
        let mut sub: StreamSubscriber<TestMsg> = StreamSubscriber::new(rx);

        // Send wrong type_id
        let bytes = vec![0u8; 10];
        let wrong_frame = RawFrame {
            type_id: 999,
            bytes: Arc::new(bytes),
        };

        let wrong_envelope = Envelope::from_raw(
            999,
            "test-stream".to_string(),
            Arc::new(wrong_frame) as Arc<dyn Any + Send + Sync>,
        );
        tx.send(wrong_envelope).unwrap();

        // Send correct type_id
        let msg = TestMsg { value: 42 };
        let correct_frame = RawFrame {
            type_id: 123,
            bytes: Arc::new(msg.as_bytes().to_vec()),
        };

        let correct_envelope = Envelope::from_raw(
            123,
            "test-stream".to_string(),
            Arc::new(correct_frame) as Arc<dyn Any + Send + Sync>,
        );
        tx.send(correct_envelope).unwrap();

        // Should receive only the correct message
        let msg = sub.recv().await.unwrap();
        assert_eq!(msg.value, 42);
    }

    #[tokio::test]
    async fn test_handle_stream_connection() {
        use crate::codec::write_message;

        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        let listener = UnixListener::bind(&socket_path).unwrap();
        let channels: Arc<DashMap<String, broadcast::Sender<Envelope>>> = Arc::new(DashMap::new());
        let (tx, mut rx) = broadcast::channel(10);
        channels.insert("test-stream".to_string(), tx);

        // Spawn server
        let server_channels = Arc::clone(&channels);
        let server_handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_stream_connection(stream, server_channels).await
        });

        // Give server time to start
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Client writes message
        let mut client = UnixStream::connect(&socket_path).await.unwrap();
        let msg = TestMsg { value: 42 };
        write_message(&mut client, &msg).await.unwrap();
        drop(client); // Close connection

        // Should receive envelope
        let envelope = tokio::time::timeout(tokio::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Timeout")
            .unwrap();

        assert_eq!(envelope.type_id, 123);

        // Server should complete
        let result = tokio::time::timeout(tokio::time::Duration::from_secs(1), server_handle)
            .await
            .expect("Server timeout");

        assert!(result.is_ok());
    }
}
