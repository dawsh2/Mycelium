use thiserror::Error;

#[derive(Error, Debug)]
pub enum TransportError {
    #[error("No subscribers available for topic")]
    NoSubscribers,

    #[error("Channel send failed")]
    SendFailed,

    #[error("Channel receive failed")]
    RecvFailed,

    #[error("Service not found: {0}")]
    ServiceNotFound(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Envelope error: {0}")]
    Envelope(#[from] mycelium_protocol::EnvelopeError),

    #[error("Codec error: {0}")]
    Codec(#[from] crate::codec::CodecError),
}

pub type Result<T> = std::result::Result<T, TransportError>;
