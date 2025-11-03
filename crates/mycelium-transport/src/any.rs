use crate::tcp::{TcpPublisher, TcpSubscriber};
use crate::unix::{UnixPublisher, UnixSubscriber};
use crate::{Publisher, Result, Subscriber};
use mycelium_protocol::Message;

/// Automatic transport-selecting publisher
///
/// Wraps any publisher type (Local/Unix/TCP) and provides a unified interface.
/// The MessageBus automatically selects the correct transport based on topology.
pub enum AnyPublisher<M: Message> {
    /// In-process Arc<T> publisher (zero-copy)
    Local(Publisher<M>),

    /// Unix socket publisher (same machine, different process)
    Unix(UnixPublisher<M>),

    /// TCP publisher (different machine)
    Tcp(TcpPublisher<M>),
}

impl<M: Message> AnyPublisher<M>
where
    M: rkyv::Serialize<rkyv::ser::serializers::AllocSerializer<1024>>,
{
    /// Publish a message using the underlying transport
    pub async fn publish(&self, msg: M) -> Result<()> {
        match self {
            AnyPublisher::Local(pub_) => pub_.publish(msg).await,
            AnyPublisher::Unix(pub_) => pub_.publish(msg).await,
            AnyPublisher::Tcp(pub_) => pub_.publish(msg).await,
        }
    }

    /// Get the transport type being used
    pub fn transport_type(&self) -> &'static str {
        match self {
            AnyPublisher::Local(_) => "local",
            AnyPublisher::Unix(_) => "unix",
            AnyPublisher::Tcp(_) => "tcp",
        }
    }
}

/// Automatic transport-selecting subscriber
///
/// Wraps any subscriber type (Local/Unix/TCP) and provides a unified interface.
/// The MessageBus automatically selects the correct transport based on topology.
pub enum AnySubscriber<M: Message> {
    /// In-process Arc<T> subscriber (zero-copy)
    Local(Subscriber<M>),

    /// Unix socket subscriber (same machine, different process)
    Unix(UnixSubscriber<M>),

    /// TCP subscriber (different machine)
    Tcp(TcpSubscriber<M>),
}

impl<M: Message + Clone> AnySubscriber<M>
where
    M::Archived: for<'a> rkyv::CheckBytes<rkyv::validation::validators::DefaultValidator<'a>>
        + rkyv::Deserialize<M, rkyv::Infallible>,
{
    /// Receive the next message using the underlying transport
    pub async fn recv(&mut self) -> Option<M> {
        match self {
            AnySubscriber::Local(sub) => {
                // Local returns Arc<M>, need to clone to get M
                sub.recv().await.map(|arc| (*arc).clone())
            }
            AnySubscriber::Unix(sub) => sub.recv().await,
            AnySubscriber::Tcp(sub) => sub.recv().await,
        }
    }

    /// Try to receive a message without blocking
    pub fn try_recv(&mut self) -> Option<M> {
        match self {
            AnySubscriber::Local(sub) => {
                // Local returns Option<Arc<M>>, need to clone to get M
                sub.try_recv().map(|arc| (*arc).clone())
            }
            AnySubscriber::Unix(_) => {
                // Unix/TCP don't have try_recv
                None
            }
            AnySubscriber::Tcp(_) => {
                // Unix/TCP don't have try_recv
                None
            }
        }
    }

    /// Get the transport type being used
    pub fn transport_type(&self) -> &'static str {
        match self {
            AnySubscriber::Local(_) => "local",
            AnySubscriber::Unix(_) => "unix",
            AnySubscriber::Tcp(_) => "tcp",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LocalTransport;
    use mycelium_protocol::impl_message;
    use rkyv::{Archive, Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Archive, Serialize, Deserialize)]
    #[archive(check_bytes)]
    struct TestMsg {
        value: u64,
    }

    impl_message!(TestMsg, 77, "test-any");

    #[tokio::test]
    async fn test_any_publisher_local() {
        let transport = LocalTransport::default();
        let pub_ = transport.publisher::<TestMsg>();
        let mut sub = transport.subscriber::<TestMsg>();

        let any_pub = AnyPublisher::Local(pub_);

        any_pub.publish(TestMsg { value: 42 }).await.unwrap();

        let msg = sub.recv().await.unwrap();
        assert_eq!(msg.value, 42);
        assert_eq!(any_pub.transport_type(), "local");
    }

    #[tokio::test]
    async fn test_any_subscriber_local() {
        let transport = LocalTransport::default();
        let pub_ = transport.publisher::<TestMsg>();
        let sub = transport.subscriber::<TestMsg>();

        let mut any_sub = AnySubscriber::Local(sub);

        pub_.publish(TestMsg { value: 99 }).await.unwrap();

        let msg = any_sub.recv().await.unwrap();
        assert_eq!(msg.value, 99);
        assert_eq!(any_sub.transport_type(), "local");
    }
}
