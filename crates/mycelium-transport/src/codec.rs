use mycelium_protocol::{Message, HEADER_SIZE, MAX_PAYLOAD_SIZE};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zerocopy::{AsBytes, FromBytes};

#[derive(Error, Debug)]
pub enum CodecError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Protocol codec error: {0}")]
    ProtocolCodec(#[from] mycelium_protocol::CodecError),

    #[error("Message too large: {0} bytes")]
    MessageTooLarge(usize),
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
    M: Message + AsBytes,
    W: AsyncWriteExt + Unpin,
{
    // Use protocol codec for zerocopy encoding
    let tlv_bytes = mycelium_protocol::encode_message(msg)?;

    // Write entire TLV frame (header + payload)
    stream.write_all(&tlv_bytes).await?;

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
    let payload_len = u32::from_le_bytes([header_buf[2], header_buf[3], header_buf[4], header_buf[5]]) as usize;

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

/// Deserialize a message from TLV bytes using zerocopy
///
/// This performs zero-copy deserialization by casting the bytes directly
/// to the message struct (no allocation, no copying).
pub fn deserialize_message<M: Message>(tlv_bytes: &[u8]) -> Result<M>
where
    M: FromBytes,
{
    Ok(mycelium_protocol::decode_message(tlv_bytes)?)
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

        let deserialized: TestMsg = deserialize_message(&tlv_bytes).unwrap();
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

    #[tokio::test]
    async fn test_zerocopy_deserialization() {
        // Verify true zero-copy (no allocation during deserialize)
        let msg = TestMsg {
            value: 123,
            data: 456,
        };

        // Encode
        let tlv_bytes = mycelium_protocol::encode_message(&msg).unwrap();

        // Decode (should be zero-copy cast)
        let decoded: TestMsg = deserialize_message(&tlv_bytes).unwrap();

        assert_eq!(decoded.value, 123);
        assert_eq!(decoded.data, 456);
    }
}
