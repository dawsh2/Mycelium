use mycelium_protocol::Message;
use rkyv::Deserialize;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Error, Debug)]
pub enum CodecError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization failed")]
    SerializationFailed,

    #[error("Deserialization failed")]
    DeserializationFailed,

    #[error("Message too large: {0} bytes")]
    MessageTooLarge(usize),
}

pub type Result<T> = std::result::Result<T, CodecError>;

/// Maximum message size (10MB)
const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024;

/// TLV frame format:
/// ```text
/// ┌──────────┬──────────┬──────────────┐
/// │ Length   │ Type ID  │ Value        │
/// │ 4 bytes  │ 2 bytes  │ N bytes      │
/// │ (u32 LE) │ (u16 LE) │ (payload)    │
/// └──────────┴──────────┴──────────────┘
/// ```

/// Write a TLV-framed message to an async stream
pub async fn write_message<M, W>(stream: &mut W, msg: &M) -> Result<()>
where
    M: Message + rkyv::Serialize<rkyv::ser::serializers::AllocSerializer<1024>>,
    W: AsyncWriteExt + Unpin,
{
    // Serialize with rkyv
    let bytes = rkyv::to_bytes::<_, 1024>(msg).map_err(|_| CodecError::SerializationFailed)?;

    let len = bytes.len();
    if len > MAX_MESSAGE_SIZE {
        return Err(CodecError::MessageTooLarge(len));
    }

    // Write TLV frame
    stream.write_all(&(len as u32).to_le_bytes()).await?;
    stream.write_all(&M::TYPE_ID.to_le_bytes()).await?;
    stream.write_all(&bytes).await?;

    Ok(())
}

/// Read a TLV-framed message from an async stream
///
/// Returns (type_id, payload_bytes)
pub async fn read_frame<R>(stream: &mut R) -> Result<(u16, Vec<u8>)>
where
    R: AsyncReadExt + Unpin,
{
    // Read length
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;

    if len > MAX_MESSAGE_SIZE {
        return Err(CodecError::MessageTooLarge(len));
    }

    // Read type ID
    let mut type_buf = [0u8; 2];
    stream.read_exact(&mut type_buf).await?;
    let type_id = u16::from_le_bytes(type_buf);

    // Read payload
    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload).await?;

    Ok((type_id, payload))
}

/// Deserialize a message from rkyv bytes
pub fn deserialize_message<M: Message>(bytes: &[u8]) -> Result<M>
where
    M::Archived: for<'a> rkyv::CheckBytes<rkyv::validation::validators::DefaultValidator<'a>>
        + rkyv::Deserialize<M, rkyv::Infallible>,
{
    let archived = unsafe { rkyv::archived_root::<M>(bytes) };
    archived
        .deserialize(&mut rkyv::Infallible)
        .map_err(|_| CodecError::DeserializationFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mycelium_protocol::impl_message;
    use rkyv::{Archive, Deserialize, Serialize};
    use tokio::net::{UnixListener, UnixStream};

    #[derive(Debug, Clone, PartialEq, Archive, Serialize, Deserialize)]
    #[archive(check_bytes)]
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
            let (type_id, bytes) = read_frame(&mut stream).await.unwrap();
            (type_id, bytes)
        });

        // Client writes message
        let mut client = UnixStream::connect(&socket_path).await.unwrap();
        let msg = TestMsg {
            value: 42,
            data: 100,
        };
        write_message(&mut client, &msg).await.unwrap();

        // Server reads message
        let (type_id, bytes) = server_handle.await.unwrap();
        assert_eq!(type_id, 42);

        let deserialized: TestMsg = deserialize_message(&bytes).unwrap();
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

        // Client writes oversized frame
        let mut client = UnixStream::connect(&socket_path).await.unwrap();
        let len = (MAX_MESSAGE_SIZE + 1) as u32;
        client.write_all(&len.to_le_bytes()).await.unwrap();
        client.write_all(&42u16.to_le_bytes()).await.unwrap();

        // Server should get an error
        let result = server_handle.await.unwrap();
        assert!(matches!(result, Err(CodecError::MessageTooLarge(_))));
    }
}
