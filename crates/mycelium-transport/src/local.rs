use crate::bridge::BridgeFanout;
use crate::codec::deserialize_message;
use crate::error::Result;
use crate::{config::TransportConfig, ChannelManager, Publisher, Subscriber, TransportError};
use dashmap::DashMap;
use mycelium_protocol::{Envelope, Message};
use std::sync::Arc;

/// Local transport using Arc<T> for zero-copy message passing
///
/// This transport is used when all services are in the same process.
/// Messages are shared via Arc, avoiding serialization overhead.
///
/// Performance: ~50-200ns per message (just an Arc clone)
#[derive(Clone)]
pub struct LocalTransport {
    channel_manager: ChannelManager,
    bridge_fanout: BridgeFanout,
    type_registry: TypeRegistry,
}

impl LocalTransport {
    /// Create a new local transport with default configuration
    pub fn new() -> Self {
        Self::with_config(TransportConfig::default())
    }

    /// Create a new local transport with custom configuration
    pub fn with_config(config: TransportConfig) -> Self {
        Self {
            channel_manager: ChannelManager::new(config.clone()),
            bridge_fanout: BridgeFanout::new(config.channel_capacity),
            type_registry: TypeRegistry::new(),
        }
    }

    /// Create a new local transport (deprecated: use with_config)
    ///
    /// # Arguments
    /// * `channel_capacity` - Capacity of each broadcast channel
    #[deprecated(since = "0.2.0", note = "Use with_config(TransportConfig) instead")]
    pub fn with_capacity(channel_capacity: usize) -> Self {
        let mut config = TransportConfig::default();
        config.channel_capacity = channel_capacity;
        Self::with_config(config)
    }

    /// Create a publisher for a message type
    pub fn publisher<M: Message>(&self) -> Publisher<M> {
        self.type_registry.register::<M>();
        let tx = self.channel_manager.get_or_create_channel::<M>();
        Publisher::with_bridge(tx, self.bridge_fanout.clone())
    }

    /// Create a subscriber for a message type
    pub fn subscriber<M: Message>(&self) -> Subscriber<M> {
        self.type_registry.register::<M>();
        let tx = self.channel_manager.get_or_create_channel::<M>();
        let rx = tx.subscribe();
        Subscriber::new(rx)
    }

    /// Create a publisher for an explicit topic (Phase 1: Actor-ready)
    ///
    /// This enables dynamic topic creation for actor mailboxes and partitioned topics.
    /// Example: `publisher_for_topic::<MyMessage>("actor.123abc")`
    pub fn publisher_for_topic<M: Message>(&self, topic: &str) -> Publisher<M> {
        self.type_registry.register::<M>();
        let tx = self.channel_manager.get_or_create_channel_for_topic(topic);
        Publisher::with_bridge(tx, self.bridge_fanout.clone())
    }

    /// Create a subscriber for an explicit topic (Phase 1: Actor-ready)
    ///
    /// This enables dynamic topic subscription for actor mailboxes and partitioned topics.
    /// Example: `subscriber_for_topic::<MyMessage>("actor.123abc")`
    pub fn subscriber_for_topic<M: Message>(&self, topic: &str) -> Subscriber<M> {
        self.type_registry.register::<M>();
        let tx = self.channel_manager.get_or_create_channel_for_topic(topic);
        let rx = tx.subscribe();
        Subscriber::new(rx)
    }

    /// Get the number of active topics
    pub fn topic_count(&self) -> usize {
        self.channel_manager.topic_count()
    }

    /// Get the number of subscribers for a topic
    pub fn subscriber_count<M: Message>(&self) -> usize {
        self.channel_manager.subscriber_count::<M>()
    }

    pub(crate) fn bridge_fanout(&self) -> BridgeFanout {
        self.bridge_fanout.clone()
    }

    pub(crate) fn build_envelope_from_payload(
        &self,
        type_id: u16,
        payload: &[u8],
    ) -> Result<Envelope> {
        self.type_registry.build(type_id, payload)
    }

    pub(crate) fn dispatch_envelope(&self, envelope: Envelope) -> Result<usize> {
        let topic = envelope.topic.clone();
        let tx = self.channel_manager.get_or_create_channel_for_topic(&topic);
        tx.send(envelope).map_err(|_| TransportError::SendFailed)
    }
}

impl Default for LocalTransport {
    fn default() -> Self {
        Self::new()
    }
}

type EnvelopeBuilder = fn(&[u8]) -> Result<Envelope>;

#[derive(Clone)]
struct TypeRegistry {
    map: Arc<DashMap<u16, EnvelopeBuilder>>,
}

impl TypeRegistry {
    fn new() -> Self {
        Self {
            map: Arc::new(DashMap::new()),
        }
    }

    fn register<M: Message>(&self) {
        self.map.entry(M::TYPE_ID).or_insert(envelope_builder::<M>);
    }

    fn build(&self, type_id: u16, payload: &[u8]) -> Result<Envelope> {
        let builder = self.map.get(&type_id).ok_or_else(|| {
            TransportError::ServiceNotFound(format!(
                "No registered message type for type_id {}",
                type_id
            ))
        })?;
        builder(payload)
    }
}

fn envelope_builder<M: Message>(payload: &[u8]) -> Result<Envelope> {
    let msg = deserialize_message::<M>(payload)?;
    Ok(Envelope::new(msg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mycelium_protocol::impl_message;
    use zerocopy::{AsBytes, FromBytes, FromZeroes};

    #[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
    #[repr(C)]
    struct TestMsg {
        value: u64,
    }

    impl_message!(TestMsg, 1, "test-topic");

    #[tokio::test]
    async fn test_local_transport_pubsub() {
        let transport = LocalTransport::new();

        let pub_ = transport.publisher::<TestMsg>();
        let mut sub = transport.subscriber::<TestMsg>();

        // Publish message
        pub_.publish(TestMsg { value: 42 }).await.unwrap();

        // Receive message
        let msg = sub.recv().await.unwrap();
        assert_eq!(msg.value, 42);
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let transport = LocalTransport::new();

        let pub_ = transport.publisher::<TestMsg>();
        let mut sub1 = transport.subscriber::<TestMsg>();
        let mut sub2 = transport.subscriber::<TestMsg>();

        // Publish message
        pub_.publish(TestMsg { value: 42 }).await.unwrap();

        // Both subscribers should receive
        let msg1 = sub1.recv().await.unwrap();
        let msg2 = sub2.recv().await.unwrap();

        assert_eq!(msg1.value, 42);
        assert_eq!(msg2.value, 42);
    }

    #[tokio::test]
    async fn test_subscriber_count() {
        let transport = LocalTransport::new();

        let _sub1 = transport.subscriber::<TestMsg>();
        let _sub2 = transport.subscriber::<TestMsg>();

        assert_eq!(transport.subscriber_count::<TestMsg>(), 2);
    }

    #[tokio::test]
    async fn test_multiple_topics() {
        #[derive(Debug, Clone, Copy, AsBytes, FromBytes, FromZeroes)]
        #[repr(C)]
        struct Msg1 {
            value: u64,
        }

        #[derive(Debug, Clone, Copy, AsBytes, FromBytes, FromZeroes)]
        #[repr(C)]
        struct Msg2 {
            data: u64,
        }

        impl_message!(Msg1, 10, "topic1");
        impl_message!(Msg2, 11, "topic2");

        let transport = LocalTransport::new();

        let pub1 = transport.publisher::<Msg1>();
        let pub2 = transport.publisher::<Msg2>();

        let mut sub1 = transport.subscriber::<Msg1>();
        let mut sub2 = transport.subscriber::<Msg2>();

        // Publish to different topics
        pub1.publish(Msg1 { value: 42 }).await.unwrap();
        pub2.publish(Msg2 { data: 100 }).await.unwrap();

        // Each subscriber only receives its own message
        let msg1 = sub1.recv().await.unwrap();
        let msg2 = sub2.recv().await.unwrap();

        assert_eq!(msg1.value, 42);
        assert_eq!(msg2.data, 100);
    }

    #[tokio::test]
    async fn test_topic_count() {
        #[derive(Debug, Clone, Copy, AsBytes, FromBytes, FromZeroes)]
        #[repr(C)]
        struct Msg1 {
            _dummy: u8, // Empty structs not supported by zerocopy
        }

        #[derive(Debug, Clone, Copy, AsBytes, FromBytes, FromZeroes)]
        #[repr(C)]
        struct Msg2 {
            _dummy: u8,
        }

        impl_message!(Msg1, 20, "topic-a");
        impl_message!(Msg2, 21, "topic-b");

        let transport = LocalTransport::new();

        let _pub1 = transport.publisher::<Msg1>();
        let _pub2 = transport.publisher::<Msg2>();

        assert_eq!(transport.topic_count(), 2);
    }
}
