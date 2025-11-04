use crate::{config::TransportConfig, Publisher, Subscriber, ChannelManager};
use mycelium_protocol::Message;

/// Local transport using Arc<T> for zero-copy message passing
///
/// This transport is used when all services are in the same process.
/// Messages are shared via Arc, avoiding serialization overhead.
///
/// Performance: ~50-200ns per message (just an Arc clone)
#[derive(Clone)]
pub struct LocalTransport {
    channel_manager: ChannelManager,
}

impl LocalTransport {
    /// Create a new local transport with default configuration
    pub fn new() -> Self {
        Self::with_config(TransportConfig::default())
    }

    /// Create a new local transport with custom configuration
    pub fn with_config(config: TransportConfig) -> Self {
        Self {
            channel_manager: ChannelManager::new(config),
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
        let tx = self.channel_manager.get_or_create_channel::<M>();
        Publisher::new(tx)
    }

    /// Create a subscriber for a message type
    pub fn subscriber<M: Message>(&self) -> Subscriber<M> {
        let tx = self.channel_manager.get_or_create_channel::<M>();
        let rx = tx.subscribe();
        Subscriber::new(rx)
    }

    /// Create a publisher for an explicit topic (Phase 1: Actor-ready)
    ///
    /// This enables dynamic topic creation for actor mailboxes and partitioned topics.
    /// Example: `publisher_for_topic::<MyMessage>("actor.123abc")`
    pub fn publisher_for_topic<M: Message>(&self, topic: &str) -> Publisher<M> {
        let tx = self.channel_manager.get_or_create_channel_for_topic(topic);
        Publisher::new(tx)
    }

    /// Create a subscriber for an explicit topic (Phase 1: Actor-ready)
    ///
    /// This enables dynamic topic subscription for actor mailboxes and partitioned topics.
    /// Example: `subscriber_for_topic::<MyMessage>("actor.123abc")`
    pub fn subscriber_for_topic<M: Message>(&self, topic: &str) -> Subscriber<M> {
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
}

impl Default for LocalTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mycelium_protocol::impl_message;
    use zerocopy::{IntoBytes, FromBytes, FromZeros, Immutable};

    #[derive(Debug, Clone, Copy, PartialEq, IntoBytes, FromBytes, FromZeros, Immutable)]
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
        #[derive(Debug, Clone, Copy, IntoBytes, FromBytes, FromZeros, Immutable)]
        #[repr(C)]
        struct Msg1 {
            value: u64,
        }

        #[derive(Debug, Clone, Copy, IntoBytes, FromBytes, FromZeros, Immutable)]
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
        pub2.publish(Msg2 {
            data: 100,
        })
        .await
        .unwrap();

        // Each subscriber only receives its own message
        let msg1 = sub1.recv().await.unwrap();
        let msg2 = sub2.recv().await.unwrap();

        assert_eq!(msg1.value, 42);
        assert_eq!(msg2.data, 100);
    }

    #[tokio::test]
    async fn test_topic_count() {
        #[derive(Debug, Clone, Copy, IntoBytes, FromBytes, FromZeros, Immutable)]
        #[repr(C)]
        struct Msg1 {
            _dummy: u8,  // Empty structs not supported by zerocopy
        }

        #[derive(Debug, Clone, Copy, IntoBytes, FromBytes, FromZeros, Immutable)]
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
