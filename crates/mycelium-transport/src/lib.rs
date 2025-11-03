pub mod any;
pub mod bounded;
pub mod bus;
pub mod codec;
pub mod config;
pub mod error;
pub mod local;
pub mod ordered;
pub mod publisher;
pub mod service;
pub mod shared;
pub mod stream;
pub mod topics;
pub mod subscriber;
pub mod tcp;
pub mod unix;

// TODO: Fix compilation errors in these advanced modules
// pub mod backpressure;
// pub mod pool;

pub use any::{AnyPublisher, AnySubscriber};
pub use bounded::{BoundedPublisher, BoundedPublisherBuilder, BoundedSubscriber};
pub use bus::MessageBus;
pub use codec::{deserialize_message, read_frame, write_message, CodecError};
pub use config::TransportConfig;
pub use error::{Result, TransportError};
pub use local::LocalTransport;
pub use ordered::{OrderedSubscriber, OrderingStats};
pub use publisher::Publisher;
pub use service::{
    DegradedReason, HealthStatus, ManagedService, ServiceRunner, UnhealthyReason,
};
pub use shared::{BufferSizes, ChannelManager, ConnectionInfo, TransportType};
pub use stream::{StreamSubscriber, handle_stream_connection};
pub use subscriber::Subscriber;
pub use topics::TopicBuilder;
pub use tcp::{TcpPublisher, TcpSubscriber, TcpTransport};
pub use unix::{UnixPublisher, UnixSubscriber, UnixTransport};
