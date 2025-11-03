pub mod any;
pub mod bus;
pub mod codec;
pub mod config;
pub mod error;
pub mod local;
pub mod publisher;
pub mod shared;
pub mod stream;
pub mod subscriber;
pub mod tcp;
pub mod unix;

// TODO: Fix compilation errors in these advanced modules
// pub mod backpressure;
// pub mod pool;

pub use any::{AnyPublisher, AnySubscriber};
pub use bus::MessageBus;
pub use codec::{deserialize_message, read_frame, write_message, CodecError};
pub use config::TransportConfig;
pub use error::{Result, TransportError};
pub use local::LocalTransport;
pub use publisher::Publisher;
pub use shared::{BufferSizes, ChannelManager, ConnectionInfo, TransportType};
pub use stream::{StreamSubscriber, handle_stream_connection};
pub use subscriber::Subscriber;
pub use tcp::{TcpPublisher, TcpSubscriber, TcpTransport};
pub use unix::{UnixPublisher, UnixSubscriber, UnixTransport};
