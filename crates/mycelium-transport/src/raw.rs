use crate::bridge::BridgeFrame;
use mycelium_protocol::codec::HEADER_SIZE;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Raw TLV frame emitted by the message bus.
///
/// This exposes the serialized bytes for foreign runtimes without requiring
/// knowledge of the concrete Rust message type.
#[derive(Clone, Debug)]
pub struct RawMessage {
    type_id: u16,
    topic: Arc<str>,
    bytes: Arc<Vec<u8>>,
}

impl RawMessage {
    /// Message type identifier (TLV tag).
    pub fn type_id(&self) -> u16 {
        self.type_id
    }

    /// Topic string associated with the message.
    pub fn topic(&self) -> &str {
        &self.topic
    }

    /// Borrow the payload bytes (skips the TLV header).
    pub fn payload(&self) -> &[u8] {
        if self.bytes.len() <= HEADER_SIZE {
            &[]
        } else {
            &self.bytes[HEADER_SIZE..]
        }
    }

    /// Take ownership of the underlying TLV buffer (type + len + payload).
    pub fn into_tlv(self) -> Arc<Vec<u8>> {
        self.bytes
    }
}

impl From<BridgeFrame> for RawMessage {
    fn from(frame: BridgeFrame) -> Self {
        Self {
            type_id: frame.type_id,
            topic: frame.topic,
            bytes: frame.bytes,
        }
    }
}

/// Broadcast receiver that yields [`RawMessage`] instances.
pub struct RawMessageStream {
    rx: broadcast::Receiver<BridgeFrame>,
}

impl RawMessageStream {
    pub(crate) fn new(rx: broadcast::Receiver<BridgeFrame>) -> Self {
        Self { rx }
    }

    /// Await the next raw frame.
    pub async fn recv(&mut self) -> Option<RawMessage> {
        loop {
            match self.rx.recv().await {
                Ok(frame) => return Some(frame.into()),
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "raw subscriber lagged");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }
}
