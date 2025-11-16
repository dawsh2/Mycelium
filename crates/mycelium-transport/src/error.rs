use thiserror::Error;

#[derive(Error, Debug)]
pub enum TransportError {
    #[error("No subscribers available for topic: {topic}")]
    NoSubscribers { topic: String },

    #[error("Channel send failed")]
    SendFailed,

    #[error("Channel receive failed")]
    RecvFailed,

    #[error("Channel is full (backpressure applied)")]
    ChannelFull,

    #[error("Channel is closed (receiver dropped)")]
    ChannelClosed,

    #[error("Operation timed out after {0:?}")]
    Timeout(std::time::Duration),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = TransportError::NoSubscribers {
            topic: "test_topic".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "No subscribers available for topic: test_topic"
        );

        let err = TransportError::SendFailed;
        assert_eq!(err.to_string(), "Channel send failed");

        let err = TransportError::ChannelFull;
        assert_eq!(err.to_string(), "Channel is full (backpressure applied)");

        let err = TransportError::ChannelClosed;
        assert_eq!(err.to_string(), "Channel is closed (receiver dropped)");

        let err = TransportError::Timeout(std::time::Duration::from_secs(5));
        assert!(err.to_string().contains("timed out"));

        let err = TransportError::InvalidConfig("buffer too small".to_string());
        assert_eq!(err.to_string(), "Invalid configuration: buffer too small");

        let err = TransportError::ServiceNotFound("my_service".to_string());
        assert_eq!(err.to_string(), "Service not found: my_service");
    }

    #[test]
    fn test_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: TransportError = io_err.into();
        assert!(matches!(err, TransportError::Io(_)));
    }

    #[test]
    fn test_result_type() {
        let ok_result: Result<i32> = Ok(42);
        assert_eq!(ok_result.unwrap(), 42);

        let err_result: Result<i32> = Err(TransportError::SendFailed);
        assert!(err_result.is_err());
    }

    #[test]
    fn test_error_debug() {
        let err = TransportError::ChannelFull;
        let debug_str = format!("{:?}", err);
        assert_eq!(debug_str, "ChannelFull");
    }
}
