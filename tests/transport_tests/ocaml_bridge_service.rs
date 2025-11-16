use crate::common::*;
use mycelium_protocol::{codec::HEADER_SIZE, FixedStr, Message, TextMessage, SCHEMA_DIGEST};
use mycelium_transport::{
    ocaml_bridge::{OcamlBridgeConfig, OcamlBridgeService, OcamlChildConfig},
    MessageBus, ServiceRuntime,
};
use serde::Deserialize;
use serde_json::from_slice;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tempfile::tempdir;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::time::{sleep, timeout, Instant};
use zerocopy::AsBytes;

#[tokio::test]
async fn test_ocaml_bridge_service_round_trip() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("bridge.sock");

    let bus = MessageBus::new();
    let config = OcamlBridgeConfig::new(&socket_path);
    let service = OcamlBridgeService::new(bus.clone(), config);
    let runtime = ServiceRuntime::new(bus.clone());
    runtime.spawn_service(service).await.expect("spawn bridge");

    wait_for_socket(&socket_path).await;

    let mut stream = UnixStream::connect(&socket_path)
        .await
        .expect("connect bridge");
    send_handshake(&mut stream).await;
    let (mut reader, mut writer) = stream.into_split();

    let client_event = create_test_swap_event(777);
    write_tlv(&mut writer, &client_event).await;
    let mut bus_sub = bus.subscriber::<SwapEvent>();
    let received = timeout(Duration::from_secs(1), bus_sub.recv())
        .await
        .expect("recv event")
        .expect("subscriber closed");
    assert_eq!(*received, client_event);

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
async fn test_ocaml_bridge_service_with_ocaml_worker() {
    if let Err(err) = build_ocaml_fixture() {
        eprintln!("skipping OCaml worker test (missing dune?): {}", err);
        return;
    }

    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("bridge.sock");
    let output_path = dir.path().join("ocaml-output.json");

    let bus = MessageBus::new();
    let mut config = OcamlBridgeConfig::new(&socket_path);
    config.child = Some(ocaml_child_config(&output_path));

    let mut text_subscriber = bus.subscriber::<TextMessage>();
    let runtime = ServiceRuntime::new(bus.clone());
    runtime
        .spawn_service(OcamlBridgeService::new(bus.clone(), config))
        .await
        .expect("spawn bridge service");

    let ocaml_message = timeout(Duration::from_secs(10), text_subscriber.recv())
        .await
        .expect("ocaml publish timed out")
        .expect("text subscriber closed");
    assert_eq!(ocaml_message.sender.as_str().unwrap(), "ocaml-writer");
    assert_eq!(ocaml_message.content.as_str().unwrap(), "hello-from-ocaml");

    let publisher = bus.publisher::<TextMessage>();
    let rust_to_ocaml = TextMessage {
        sender: FixedStr::<32>::from_str("rust-service").expect("sender fits"),
        content: FixedStr::<256>::from_str("hello-ocaml-client").expect("content fits"),
        timestamp: 424242,
    };
    publisher
        .publish(rust_to_ocaml)
        .await
        .expect("publish rust -> ocaml");

    wait_for_file(&output_path, Duration::from_secs(10))
        .await
        .expect("ocaml output");
    let data = fs::read(&output_path).await.expect("read ocaml output");
    let summary: OcamlBridgeSummary = from_slice(&data).expect("parse ocaml output");
    assert_eq!(summary.outbound.sender, "ocaml-writer");
    assert_eq!(summary.outbound.content, "hello-from-ocaml");
    let inbound = summary.inbound.expect("ocaml should receive rust message");
    assert_eq!(inbound.sender, "rust-service");
    assert_eq!(inbound.content, "hello-ocaml-client");
    assert_eq!(inbound.timestamp.round() as u64, 424242);

    runtime.shutdown().await.expect("shutdown runtime");
}

#[derive(Debug, Deserialize)]
struct OcamlBridgeSummary {
    outbound: OcamlMessage,
    inbound: Option<OcamlMessage>,
}

#[derive(Debug, Deserialize)]
struct OcamlMessage {
    sender: String,
    content: String,
    timestamp: f64,
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

async fn wait_for_socket(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if path.exists() {
            return;
        }
        if Instant::now() >= deadline {
            panic!("socket never appeared: {}", path.display());
        }
        sleep(Duration::from_millis(10)).await;
    }
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

fn ocaml_child_config(output_path: &Path) -> OcamlChildConfig {
    let mut child = OcamlChildConfig::default();
    child.program = ocaml_fixture_path().display().to_string();
    child.env.push((
        "MYCELIUM_TEST_OUTPUT".into(),
        output_path.display().to_string(),
    ));
    child
}

fn ocaml_fixture_path() -> PathBuf {
    let with_ext = PathBuf::from("ocaml-sdk/_build/default/test/ocaml_bridge_echo.exe");
    if with_ext.exists() {
        with_ext
    } else {
        PathBuf::from("ocaml-sdk/_build/default/test/ocaml_bridge_echo")
    }
}

fn build_ocaml_fixture() -> std::io::Result<()> {
    let status = Command::new("dune")
        .arg("build")
        .arg("test/ocaml_bridge_echo.exe")
        .current_dir("ocaml-sdk")
        .status()?;
    if !status.success() {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "dune build failed",
        ))
    } else {
        Ok(())
    }
}
