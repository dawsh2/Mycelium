use crate::common::*;
use mycelium_protocol::{codec::HEADER_SIZE, FixedStr, Message, TextMessage, SCHEMA_DIGEST};
use mycelium_transport::{
    python_bridge::{PythonBridgeConfig, PythonChildConfig},
    MessageBus, PythonBridgeService, ServiceRuntime,
};
use serde::Deserialize;
use serde_json::from_slice;
use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::tempdir;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::time::{sleep, timeout, Instant};
use zerocopy::AsBytes;

#[tokio::test]
async fn test_python_bridge_service_round_trip() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("bridge.sock");

    let bus = MessageBus::new();
    let config = PythonBridgeConfig::new(&socket_path);
    let service = PythonBridgeService::new(bus.clone(), config);
    let runtime = ServiceRuntime::new(bus.clone());
    runtime.spawn_service(service).await.expect("spawn bridge");

    // Wait for socket to appear
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            if socket_path.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("socket ready");

    let mut stream = UnixStream::connect(&socket_path)
        .await
        .expect("connect bridge");
    send_handshake(&mut stream).await;
    let (mut reader, mut writer) = stream.into_split();

    // Test client → bridge → bus
    let client_event = create_test_swap_event(777);
    write_tlv(&mut writer, &client_event).await;
    let mut bus_sub = bus.subscriber::<SwapEvent>();
    let received = tokio::time::timeout(std::time::Duration::from_secs(1), bus_sub.recv())
        .await
        .expect("recv event")
        .expect("subscriber closed");
    assert_eq!(*received, client_event);

    // Test bus → bridge → client
    let bus_event = create_test_swap_event(999);
    let bus_pub = bus.publisher::<SwapEvent>();
    bus_pub.publish(bus_event).await.expect("publish");

    let mut header = [0u8; HEADER_SIZE];
    reader.read_exact(&mut header).await.expect("read header");
    let payload_len = u32::from_le_bytes([header[2], header[3], header[4], header[5]]) as usize;
    let mut payload = vec![0u8; payload_len];
    reader.read_exact(&mut payload).await.expect("read payload");
    let decoded =
        mycelium_transport::deserialize_message::<SwapEvent>(&payload).expect("decode payload");
    assert_eq!(decoded.pool_id, 999);

    runtime.shutdown().await.expect("shutdown bridge");
}

#[tokio::test]
async fn test_python_bridge_service_with_python_sdk_client() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("bridge.sock");
    let output_path = dir.path().join("python-output.json");

    let bus = MessageBus::new();
    let mut config = PythonBridgeConfig::new(&socket_path);
    config.child = Some(python_child_config(&output_path));

    let mut text_subscriber = bus.subscriber::<TextMessage>();
    let runtime = ServiceRuntime::new(bus.clone());
    runtime
        .spawn_service(PythonBridgeService::new(bus.clone(), config))
        .await
        .expect("spawn bridge service");

    // Verify Python client publishes into the bus.
    let python_message = timeout(Duration::from_secs(10), text_subscriber.recv())
        .await
        .expect("python publish timed out")
        .expect("text subscriber closed");
    assert_eq!(python_message.sender.as_str().unwrap(), "python-writer");
    assert_eq!(
        python_message.content.as_str().unwrap(),
        "hello-from-python"
    );

    // Send a message from Rust to Python and wait for the script to capture it.
    let publisher = bus.publisher::<TextMessage>();
    let rust_to_python = TextMessage {
        sender: FixedStr::<32>::from_str("rust-service").expect("sender fits"),
        content: FixedStr::<256>::from_str("hello-python-client").expect("content fits"),
        timestamp: 424242,
    };
    publisher
        .publish(rust_to_python)
        .await
        .expect("publish rust -> python");

    wait_for_file(&output_path, Duration::from_secs(10))
        .await
        .expect("python output file created");
    let data = fs::read(&output_path).await.expect("read python output");
    let summary: PythonBridgeSummary = from_slice(&data).expect("parse python output");

    assert_eq!(summary.outbound.sender, "python-writer");
    assert_eq!(summary.outbound.content, "hello-from-python");
    let inbound = summary.inbound.expect("python should receive rust message");
    assert_eq!(inbound.sender, "rust-service");
    assert_eq!(inbound.content, "hello-python-client");
    assert_eq!(inbound.timestamp, 424242);

    runtime.shutdown().await.expect("shutdown runtime");
}

async fn send_handshake(stream: &mut UnixStream) {
    let len = SCHEMA_DIGEST.len() as u16;
    stream
        .write_all(&len.to_le_bytes())
        .await
        .expect("handshake len");
    stream
        .write_all(&SCHEMA_DIGEST)
        .await
        .expect("handshake bytes");
}

async fn write_tlv(writer: &mut tokio::net::unix::OwnedWriteHalf, msg: &SwapEvent) {
    let mut frame = Vec::with_capacity(HEADER_SIZE + std::mem::size_of::<SwapEvent>());
    frame.extend_from_slice(&SwapEvent::TYPE_ID.to_le_bytes());
    frame.extend_from_slice(&(std::mem::size_of::<SwapEvent>() as u32).to_le_bytes());
    frame.extend_from_slice(msg.as_bytes());
    writer.write_all(&frame).await.expect("write frame");
}

#[derive(Debug, Deserialize)]
struct PythonBridgeSummary {
    outbound: PythonMessage,
    inbound: Option<PythonMessage>,
}

#[derive(Debug, Deserialize)]
struct PythonMessage {
    sender: String,
    content: String,
    timestamp: u64,
}

fn python_child_config(output_path: &Path) -> PythonChildConfig {
    let mut child = PythonChildConfig::default();
    child.program = "python3".to_string();

    let script = repo_root().join("tests/fixtures/python_bridge_echo.py");
    assert!(
        script.exists(),
        "python test script missing: {}",
        script.display()
    );
    child.args = vec![script.to_string_lossy().to_string()];

    let python_sdk = repo_root().join("python-sdk");
    let existing_pythonpath = env::var("PYTHONPATH").unwrap_or_default();
    let pythonpath = if existing_pythonpath.is_empty() {
        python_sdk.to_string_lossy().to_string()
    } else {
        format!("{}:{}", python_sdk.display(), existing_pythonpath)
    };

    child.env.push(("PYTHONPATH".into(), pythonpath));
    child.env.push((
        "MYCELIUM_TEST_OUTPUT".into(),
        output_path.display().to_string(),
    ));
    child.env.push(("PYTHONUNBUFFERED".into(), "1".into()));
    child
}

async fn wait_for_file(path: &Path, timeout_duration: Duration) -> std::io::Result<()> {
    let deadline = Instant::now() + timeout_duration;
    loop {
        if path.exists() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("timed out waiting for {}", path.display()),
            ));
        }
        sleep(Duration::from_millis(50)).await;
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
