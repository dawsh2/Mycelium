extern crate self as mycelium_transport;

pub mod actor;
pub mod any;
pub mod bounded;
mod bridge;
pub mod buffer_pool;
pub mod bus;
pub mod codec;
pub mod config;
pub mod error;
pub mod handler;
pub mod local;
pub mod ocaml_bridge;
pub mod ordered;
pub mod publisher;
pub mod python_bridge;
pub mod qos;
pub mod raw;
pub mod service;
pub mod service_context;
pub mod service_metrics;
pub mod service_runtime;
pub mod shared;
mod socket_endpoint;
pub mod stream;
mod stream_transport;
pub mod subscriber;
pub mod tcp;
pub mod topics;
pub mod unix;
pub mod zerocopy;

// Note: These advanced modules are available but not currently exported due to compilation issues.
// They require refactoring to work with the current API (mpsc doesn't expose .len(), missing config types, etc.)
// Future work: Fix and re-export these modules for advanced use cases.
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
pub use handler::{AsyncMessageHandler, MessageHandler};
pub use local::LocalTransport;
pub use ocaml_bridge::{OcamlBridgeConfig, OcamlBridgeService, OcamlChildConfig};
pub use ordered::{OrderedSubscriber, OrderingStats};
pub use publisher::Publisher;
pub use python_bridge::{PythonBridgeConfig, PythonBridgeService, PythonChildConfig};
pub use qos::{DropPolicy, MailboxMetrics, SpawnOptions};
pub use raw::{RawMessage, RawMessageStream};
pub use service::{DegradedReason, HealthStatus, ManagedService, ServiceRunner, UnhealthyReason};
pub use service_context::ServiceContext;
pub use service_metrics::ServiceMetrics;
pub use service_runtime::{Service, ServiceHandle, ServiceRuntime};

// Re-export proc macros
pub use mycelium_macro::{routing_config, service};
pub use shared::{BufferSizes, ChannelManager, ConnectionInfo, TransportType};
pub use socket_endpoint::SocketEndpointHandle;
pub use stream::{handle_stream_connection, StreamSubscriber};
pub use subscriber::Subscriber;
pub use tcp::{TcpPublisher, TcpSubscriber, TcpTransport};
pub use topics::TopicBuilder;
pub use unix::{UnixPublisher, UnixSubscriber, UnixTransport};
pub use zerocopy::{Cow, OwnedBuf, SharedView};
