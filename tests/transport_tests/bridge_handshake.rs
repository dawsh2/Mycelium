//! Tests for socket endpoint handshake logic.

use crate::common::*;
use mycelium_protocol::{codec::HEADER_SIZE, Message, SCHEMA_DIGEST};
use mycelium_transport::MessageBus;
use std::mem;
use tempfile::tempdir;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use zerocopy::AsBytes;

#[tokio::test]
async fn test_unix_endpoint_schema_handshake_roundtrip() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("bridge.sock");

    let bus = MessageBus::new();
    let _handle = bus
        .bind_unix_endpoint_with_digest(&socket_path, SCHEMA_DIGEST)
        .await
        .expect("bind endpoint");

    let mut subscriber = bus.subscriber::<SwapEvent>();

    let mut stream = UnixStream::connect(&socket_path)
        .await
        .expect("connect unix socket");

    // Handshake: length + digest bytes
    let digest_len = SCHEMA_DIGEST.len() as u16;
    stream
        .write_all(&digest_len.to_le_bytes())
        .await
        .expect("send handshake len");
    stream
        .write_all(&SCHEMA_DIGEST)
        .await
        .expect("send handshake bytes");

    // Compose TLV frame for SwapEvent
    let event = create_test_swap_event(42);
    let mut frame = Vec::with_capacity(HEADER_SIZE + mem::size_of::<SwapEvent>());
    frame.extend_from_slice(&SwapEvent::TYPE_ID.to_le_bytes());
    frame.extend_from_slice(&(mem::size_of::<SwapEvent>() as u32).to_le_bytes());
    frame.extend_from_slice(event.as_bytes());
    stream.write_all(&frame).await.expect("send tlv frame");

    let received = subscriber.recv().await.expect("receive swap event");
    assert_eq!(*received, event);
}
