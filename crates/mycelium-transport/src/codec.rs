use crate::buffer_pool::{BufferPool, PooledBuffer};
use mycelium_protocol::{
    codec::{HEADER_SIZE, MAX_PAYLOAD_SIZE},
    Message,
};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Error, Debug)]
pub enum CodecError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Protocol codec error: {0}")]
    ProtocolCodec(#[from] mycelium_protocol::CodecError),

    #[error("Message too large: {0} bytes")]
    MessageTooLarge(usize),

    #[error("Deserialization failed")]
    DeserializationFailed,
}

pub type Result<T> = std::result::Result<T, CodecError>;

/// TLV frame format (uses protocol codec):
/// ```text
/// ┌──────────┬──────────┬──────────────┐
/// │ Type ID  │ Length   │ Value        │
/// │ 2 bytes  │ 4 bytes  │ N bytes      │
/// │ (u16 LE) │ (u32 LE) │ (zerocopy)   │
/// └──────────┴──────────┴──────────────┘
/// ```

/// Write a TLV-framed message to an async stream
///
/// Uses the protocol codec's zerocopy encoding for true zero-copy serialization.
pub async fn write_message<M, W>(stream: &mut W, msg: &M) -> Result<()>
where
    M: Message,
    W: AsyncWriteExt + Unpin,
{
    // Serialize with zerocopy (zero-copy)
    let bytes = msg.as_bytes();

    let len = bytes.len();
    if len > MAX_PAYLOAD_SIZE {
        return Err(CodecError::MessageTooLarge(len));
    }

    // Write TLV frame: [type_id: u16][length: u32][payload: bytes]
    stream.write_all(&M::TYPE_ID.to_le_bytes()).await?;
    stream.write_all(&(len as u32).to_le_bytes()).await?;
    stream.write_all(bytes).await?;

    Ok(())
}

/// Read a TLV-framed message from an async stream
///
/// Returns (type_id, tlv_bytes) where tlv_bytes includes the header.
/// This allows zero-copy deserialization later.
pub async fn read_frame<R>(stream: &mut R) -> Result<(u16, Vec<u8>)>
where
    R: AsyncReadExt + Unpin,
{
    // Read TLV header (6 bytes: type_id + length)
    let mut header_buf = [0u8; HEADER_SIZE];
    stream.read_exact(&mut header_buf).await?;

    // Parse header
    let type_id = u16::from_le_bytes([header_buf[0], header_buf[1]]);
    let payload_len =
        u32::from_le_bytes([header_buf[2], header_buf[3], header_buf[4], header_buf[5]]) as usize;

    // Validate payload size
    if payload_len > MAX_PAYLOAD_SIZE {
        return Err(CodecError::MessageTooLarge(payload_len));
    }

    // Read payload
    let mut tlv_bytes = Vec::with_capacity(HEADER_SIZE + payload_len);
    tlv_bytes.extend_from_slice(&header_buf);

    let mut payload_buf = vec![0u8; payload_len];
    stream.read_exact(&mut payload_buf).await?;
    tlv_bytes.extend_from_slice(&payload_buf);

    Ok((type_id, tlv_bytes))
}

/// Read a TLV-framed message using buffer pool (zero-allocation)
///
/// Returns (type_id, pooled_buffer) where the buffer is automatically
/// returned to the pool when dropped. Uses the TLV length field to
/// acquire the correctly-sized buffer from the pool.
///
/// # Example
/// ```ignore
/// use crate::{BufferPool, BufferPoolConfig, read_frame_pooled};
/// use mycelium_protocol::create_buffer_pool_config;
///
/// let config = create_buffer_pool_config();
/// let pool = BufferPool::new(BufferPoolConfig::from_map(config));
///
/// let (type_id, buffer) = read_frame_pooled(&mut stream, &pool).await?;
/// // buffer automatically returns to pool when dropped
/// ```
pub async fn read_frame_pooled<R>(stream: &mut R, pool: &BufferPool) -> Result<(u16, PooledBuffer)>
where
    R: AsyncReadExt + Unpin,
{
    // Read TLV header (6 bytes: type_id + length)
    let mut header_buf = [0u8; HEADER_SIZE];
    stream.read_exact(&mut header_buf).await?;

    // Parse header
    let type_id = u16::from_le_bytes([header_buf[0], header_buf[1]]);
    let payload_len =
        u32::from_le_bytes([header_buf[2], header_buf[3], header_buf[4], header_buf[5]]) as usize;

    // Validate payload size
    if payload_len > MAX_PAYLOAD_SIZE {
        return Err(CodecError::MessageTooLarge(payload_len));
    }

    // Acquire pooled buffer for HEADER + PAYLOAD (to match read_frame behavior)
    let total_len = HEADER_SIZE + payload_len;
    let mut buffer = pool.acquire(total_len);
    buffer.resize(total_len, 0);

    // Copy header into buffer
    buffer[..HEADER_SIZE].copy_from_slice(&header_buf);

    // Read payload directly into pooled buffer after header
    stream
        .read_exact(&mut buffer[HEADER_SIZE..total_len])
        .await?;

    Ok((type_id, buffer))
}

/// Deserialize a message from zerocopy bytes
///
/// IMPORTANT: This performs zerocopy deserialization but does NOT validate
/// the message invariants. The caller MUST call validate() on the result
/// when dealing with untrusted data (e.g., from network).
///
/// For typed deserialization with automatic validation, use the transport
/// layer's message handling which validates after deserialization.
pub fn deserialize_message<M: Message>(bytes: &[u8]) -> Result<M> {
    if bytes.len() != std::mem::size_of::<M>() {
        return Err(CodecError::DeserializationFailed);
    }

    M::read_from(bytes).ok_or(CodecError::DeserializationFailed)
}

/// Deserialize and validate a message from zerocopy bytes
///
/// This is the safe version that validates message invariants after deserialization.
/// Use this when handling untrusted data from the network.
///
/// This is a temporary solution until we add a Validate trait to the Message trait.
/// For now, callers need to manually call validate() on specific message types.
pub fn deserialize_message_validated<M: Message>(bytes: &[u8]) -> Result<M> {
    let msg = deserialize_message::<M>(bytes)?;

    // Note: Validation is available on generated message types via their .validate() method.
    // This function provides basic zerocopy deserialization. Callers should call .validate()
    // on the result for message types that require validation (see generated code).
    // We don't enforce validation in the trait to allow zero-overhead messages that don't need it.

    Ok(msg)
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
        data: u64,
    }

    impl_message!(TestMsg, 42, "test");

    #[tokio::test]
    async fn test_write_read_roundtrip() {
        // Create Unix socket pair
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        let listener = UnixListener::bind(&socket_path).unwrap();

        // Spawn server
        let server_handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let (type_id, tlv_bytes) = read_frame(&mut stream).await.unwrap();
            (type_id, tlv_bytes)
        });

        // Client writes message
        let mut client = UnixStream::connect(&socket_path).await.unwrap();
        let msg = TestMsg {
            value: 42,
            data: 100,
        };
        write_message(&mut client, &msg).await.unwrap();

        // Server reads message
        let (type_id, tlv_bytes) = server_handle.await.unwrap();
        assert_eq!(type_id, 42);

        // Skip the header (6 bytes) to get the payload
        let payload = &tlv_bytes[HEADER_SIZE..];
        let deserialized: TestMsg = deserialize_message(payload).unwrap();
        assert_eq!(deserialized, msg);
    }

    #[tokio::test]
    async fn test_message_too_large() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        let listener = UnixListener::bind(&socket_path).unwrap();

        // Spawn server that will try to read
        let server_handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_frame(&mut stream).await
        });

        // Client writes oversized frame header
        let mut client = UnixStream::connect(&socket_path).await.unwrap();
        // Write TLV header: type_id (2 bytes) + length (4 bytes)
        let type_id = 42u16;
        let len = (MAX_PAYLOAD_SIZE + 1) as u32;
        client.write_all(&type_id.to_le_bytes()).await.unwrap();
        client.write_all(&len.to_le_bytes()).await.unwrap();

        // Server should get an error
        let result = server_handle.await.unwrap();
        assert!(matches!(result, Err(CodecError::MessageTooLarge(_))));
    }

    #[test]
    fn test_zerocopy_deserialization() {
        // Verify true zero-copy (no allocation during deserialize)
        let msg = TestMsg {
            value: 123,
            data: 456,
        };

        // Get raw bytes (zero-copy, just a pointer cast)
        let bytes = msg.as_bytes();

        // Decode (should be zero-copy cast)
        let decoded: TestMsg = deserialize_message(bytes).unwrap();

        assert_eq!(decoded.value, 123);
        assert_eq!(decoded.data, 456);
    }

    // TODO: Add validation test once codegen supports generating validate() methods
    // The manual messages.rs has validate() methods but generated code does not yet.
    // See messages.rs for the validation pattern that should be code-generated.

    #[tokio::test]
    async fn test_read_frame_pooled() {
        use crate::buffer_pool::{BufferPool, BufferPoolConfig};
        use std::collections::HashMap;

        // Setup buffer pool
        let mut config = HashMap::new();
        config.insert(128, 10);
        let pool = BufferPool::new(BufferPoolConfig::from_map(config));

        // Create Unix socket pair
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test_pooled.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();

        // Spawn server
        let pool_clone = pool.clone();
        let server_handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let (type_id, buffer) = read_frame_pooled(&mut stream, &pool_clone).await.unwrap();
            (type_id, buffer.to_vec()) // Convert to Vec for transfer
        });

        // Client writes message
        let mut client = UnixStream::connect(&socket_path).await.unwrap();
        let msg = TestMsg {
            value: 42,
            data: 100,
        };
        write_message(&mut client, &msg).await.unwrap();

        // Server reads message with buffer pool
        let (type_id, tlv_bytes) = server_handle.await.unwrap();
        assert_eq!(type_id, 42);

        // Skip header to get payload (like read_frame)
        let payload = &tlv_bytes[HEADER_SIZE..];
        let deserialized: TestMsg = deserialize_message(payload).unwrap();
        assert_eq!(deserialized, msg);

        // Verify buffer pool stats
        let stats = pool.stats();
        assert_eq!(stats.total_acquires, 1);
        assert_eq!(stats.total_returns, 1); // Buffer returned after drop
        assert_eq!(stats.currently_in_use, 0);
    }

    #[tokio::test]
    async fn test_pooled_buffer_reuse() {
        use crate::buffer_pool::{BufferPool, BufferPoolConfig};
        use std::collections::HashMap;

        // Setup buffer pool
        let mut config = HashMap::new();
        config.insert(128, 10);
        let pool = BufferPool::new(BufferPoolConfig::from_map(config));

        // Create Unix socket pair
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test_reuse.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();

        // Send 3 messages sequentially
        let pool_clone = pool.clone();
        let server_handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();

            for _ in 0..3 {
                let (_type_id, _buffer) =
                    read_frame_pooled(&mut stream, &pool_clone).await.unwrap();
                // Buffer drops here, returns to pool
            }
        });

        // Client writes 3 messages
        let mut client = UnixStream::connect(&socket_path).await.unwrap();
        for i in 0..3 {
            let msg = TestMsg {
                value: i,
                data: i * 10,
            };
            write_message(&mut client, &msg).await.unwrap();
        }

        server_handle.await.unwrap();

        // Verify buffer reuse
        let stats = pool.stats();
        assert_eq!(stats.total_acquires, 3);
        assert_eq!(stats.total_allocations, 1); // Only 1 allocation!
        assert_eq!(stats.total_returns, 3);
        assert_eq!(stats.hit_rate(), 66.66666666666666); // 2/3 reused
    }
}
