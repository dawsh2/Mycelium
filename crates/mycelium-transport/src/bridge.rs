use mycelium_protocol::{codec::HEADER_SIZE, Message};
use std::sync::Arc;
use tokio::sync::broadcast;

/// Broadcast fanout for raw TLV frames emitted by the local transport.
///
/// Every publisher on the local MessageBus can tap into this fanout so that
/// socket endpoints (Unix/TCP) can mirror the same TLV stream to external
/// subscribers without duplicating serialization logic in downstream crates.
#[derive(Clone)]
pub(crate) struct BridgeFanout {
    tx: broadcast::Sender<BridgeFrame>,
}

impl BridgeFanout {
    pub fn new(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<BridgeFrame> {
        self.tx.subscribe()
    }

    pub fn has_subscribers(&self) -> bool {
        self.tx.receiver_count() > 0
    }

    pub fn send(&self, frame: BridgeFrame) {
        let _ = self.tx.send(frame);
    }
}

/// Wrapper around a TLV frame (type_id + length + payload) represented as an
/// Arc<Vec<u8>> so multiple socket connections can share the same bytes without
/// additional copies.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct BridgeFrame {
    pub type_id: u16,
    pub topic: Arc<str>,
    pub bytes: Arc<Vec<u8>>,
}

impl BridgeFrame {
    pub fn from_message<M: Message>(msg: &M) -> Self {
        let payload = msg.as_bytes();
        let payload_len = payload.len();

        let mut buffer = Vec::with_capacity(HEADER_SIZE + payload_len);
        buffer.extend_from_slice(&M::TYPE_ID.to_le_bytes());
        buffer.extend_from_slice(&(payload_len as u32).to_le_bytes());
        buffer.extend_from_slice(payload);

        Self {
            type_id: M::TYPE_ID,
            topic: Arc::from(M::TOPIC),
            bytes: Arc::new(buffer),
        }
    }
}
