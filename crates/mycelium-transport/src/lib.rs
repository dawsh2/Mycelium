pub mod actor;
pub mod any;
pub mod bounded;
pub mod buffer_pool;
pub mod bus;
pub mod codec;
pub mod config;
pub mod error;
pub mod local;
pub mod ordered;
pub mod publisher;
pub mod qos;
pub mod service;
pub mod service_context;
pub mod service_metrics;
pub mod service_runtime;
pub mod shared;
pub mod stream;
pub mod subscriber;
pub mod tcp;
pub mod topics;
pub mod unix;
pub mod zerocopy;

// TODO: Fix compilation errors in these advanced modules
// pub mod backpressure;
// pub mod pool;

pub use actor::{
    Actor, ActorContext, ActorRef, ActorRuntime, RestartStrategy, SpawnError, SupervisionStrategy,
};
pub use any::{AnyPublisher, AnySubscriber};
pub use bounded::{BoundedPublisher, BoundedPublisherBuilder, BoundedSubscriber};
pub use buffer_pool::{BufferPool, BufferPoolConfig};
pub use bus::MessageBus;
pub use codec::{deserialize_message, read_frame, write_message, CodecError};
pub use config::TransportConfig;
pub use error::{Result, TransportError};
pub use local::LocalTransport;
pub use ordered::{OrderedSubscriber, OrderingStats};
pub use publisher::Publisher;
pub use qos::{DropPolicy, MailboxMetrics, SpawnOptions};
pub use service::{DegradedReason, HealthStatus, ManagedService, ServiceRunner, UnhealthyReason};
pub use service_context::ServiceContext;
pub use service_metrics::ServiceMetrics;
pub use service_runtime::{Service, ServiceHandle, ServiceRuntime};

// Re-export the #[service] macro
pub use mycelium_macro::service;
pub use shared::{BufferSizes, ChannelManager, ConnectionInfo, TransportType};
pub use stream::{handle_stream_connection, StreamSubscriber};
pub use subscriber::Subscriber;
pub use tcp::{TcpPublisher, TcpSubscriber, TcpTransport};
pub use topics::TopicBuilder;
pub use unix::{UnixPublisher, UnixSubscriber, UnixTransport};
pub use zerocopy::{Cow, OwnedBuf, SharedView};
