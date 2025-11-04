use crate::buffer_pool::BufferPool;
use crate::codec::{deserialize_message, read_frame, read_frame_pooled};
use crate::error::Result;
use dashmap::DashMap;
use mycelium_protocol::{codec::HEADER_SIZE, Envelope, Message};
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
/// Reads TLV frames from the stream and broadcasts them to the appropriate channel
/// based on type_id → topic mapping. This avoids broadcasting to all channels.
///
/// When `buffer_pool` is provided, uses pooled buffers for reading (reduces allocations).
pub async fn handle_stream_connection<S>(
    mut stream: S,
    channels: Arc<DashMap<String, broadcast::Sender<Envelope>>>,
    type_to_topic: Arc<DashMap<u16, String>>,
    buffer_pool: Option<&BufferPool>,
) -> Result<()>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin + Send,
{
    loop {
        // Read TLV frame from stream (use buffer pool if available)
        let (type_id, bytes) = if let Some(pool) = buffer_pool {
            // Use pooled reading for zero allocations (after warmup)
            match read_frame_pooled(&mut stream, pool).await {
                Ok((type_id, buffer)) => {
                    // Convert pooled buffer to Vec for Arc sharing
                    // Buffer returns to pool after this line
                    (type_id, buffer.to_vec())
                }
                Err(e) => {
                    tracing::debug!("Connection closed: {}", e);
                    return Ok(());
                }
            }
        } else {
            // Fall back to regular reading
            match read_frame(&mut stream).await {
                Ok(frame) => frame,
                Err(e) => {
                    tracing::debug!("Connection closed: {}", e);
                    return Ok(());
                }
            }
        };

        // Store raw bytes in Arc for zero-copy sharing across subscribers
        let frame = RawFrame {
            type_id,
            bytes: Arc::new(bytes),
        };

        // Look up topic for this type_id
        if let Some(topic_entry) = type_to_topic.get(&type_id) {
            let topic = topic_entry.value().clone();
            drop(topic_entry); // Release the read lock

            // Send only to the matching channel (not all channels!)
            if let Some(channel) = channels.get(&topic) {
                let envelope = Envelope::from_raw(
                    frame.type_id,
                    Arc::from(topic.as_str()),
                    Arc::new(frame) as Arc<dyn Any + Send + Sync>,
                );

                let _ = channel.value().send(envelope);
            }
        } else {
            // Unknown type_id - no subscriber registered for this message type
            tracing::warn!("Received message with unknown type_id: {}", type_id);
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

            // Deserialize from raw bytes (skip header to get payload)
            let payload = &raw_frame.bytes[HEADER_SIZE..];
            match deserialize_message::<M>(payload) {
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
    use tokio::net::{UnixListener, UnixStream};
    use zerocopy::{AsBytes, FromBytes, FromZeroes};

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

        // Create a raw frame with TLV header
        let msg = TestMsg { value: 42 };
        let payload = msg.as_bytes();

        // Build TLV bytes: [type_id: u16][length: u32][payload]
        let mut tlv_bytes = Vec::with_capacity(HEADER_SIZE + payload.len());
        tlv_bytes.extend_from_slice(&123u16.to_le_bytes());
        tlv_bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        tlv_bytes.extend_from_slice(payload);

        let frame = RawFrame {
            type_id: 123,
            bytes: Arc::new(tlv_bytes),
        };

        // Send envelope with raw frame
        let envelope = Envelope::from_raw(
            123,
            Arc::from("test-stream"),
            Arc::new(frame) as Arc<dyn Any + Send + Sync>,
        );
        tx.send(envelope).unwrap();

        // Receive and verify
        let received_msg = sub.recv().await.unwrap();
        assert_eq!(received_msg.value, 42);
    }

    #[tokio::test]
    async fn test_stream_subscriber_filters_by_type() {
        let (tx, rx) = broadcast::channel(10);
        let mut sub: StreamSubscriber<TestMsg> = StreamSubscriber::new(rx);

        // Send wrong type_id (with TLV header)
        let wrong_payload = vec![0u8; 10];
        let mut wrong_tlv = Vec::with_capacity(HEADER_SIZE + 10);
        wrong_tlv.extend_from_slice(&999u16.to_le_bytes());
        wrong_tlv.extend_from_slice(&10u32.to_le_bytes());
        wrong_tlv.extend_from_slice(&wrong_payload);

        let wrong_frame = RawFrame {
            type_id: 999,
            bytes: Arc::new(wrong_tlv),
        };

        let wrong_envelope = Envelope::from_raw(
            999,
            Arc::from("test-stream"),
            Arc::new(wrong_frame) as Arc<dyn Any + Send + Sync>,
        );
        tx.send(wrong_envelope).unwrap();

        // Send correct type_id (with TLV header)
        let msg = TestMsg { value: 42 };
        let payload = msg.as_bytes();
        let mut correct_tlv = Vec::with_capacity(HEADER_SIZE + payload.len());
        correct_tlv.extend_from_slice(&123u16.to_le_bytes());
        correct_tlv.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        correct_tlv.extend_from_slice(payload);

        let correct_frame = RawFrame {
            type_id: 123,
            bytes: Arc::new(correct_tlv),
        };

        let correct_envelope = Envelope::from_raw(
            123,
            Arc::from("test-stream"),
            Arc::new(correct_frame) as Arc<dyn Any + Send + Sync>,
        );
        tx.send(correct_envelope).unwrap();

        // Should receive only the correct message
        let received_msg = sub.recv().await.unwrap();
        assert_eq!(received_msg.value, 42);
    }

    #[tokio::test]
    async fn test_handle_stream_connection() {
        use crate::codec::write_message;

        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        let listener = UnixListener::bind(&socket_path).unwrap();
        let channels: Arc<DashMap<String, broadcast::Sender<Envelope>>> = Arc::new(DashMap::new());
        let type_to_topic: Arc<DashMap<u16, String>> = Arc::new(DashMap::new());
        let (tx, mut rx) = broadcast::channel(10);
        channels.insert("test-stream".to_string(), tx);
        type_to_topic.insert(123, "test-stream".to_string());

        // Spawn server
        let server_channels = Arc::clone(&channels);
        let server_type_to_topic = Arc::clone(&type_to_topic);
        let server_handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_stream_connection(stream, server_channels, server_type_to_topic, None).await
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
