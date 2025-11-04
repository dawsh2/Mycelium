//! Routing types for actor system and request/reply patterns.
//!
//! These types are optional metadata used for:
//! - Actor-based unicast routing (Destination::Unicast)
//! - Partition-based load balancing (Destination::Partition)
//! - Request/reply correlation (CorrelationId)
//! - Distributed tracing (TraceId)
//!
//! Note: These are zero-cost when unused (Option<T> optimizes to discriminant only).

/// Routing hint for message delivery.
///
/// Used by the transport layer to determine how to route messages:
/// - Broadcast: Send to all subscribers (default pub/sub behavior)
/// - Unicast: Send to specific actor instance (actor mailbox)
/// - Partition: Route based on hash value (load balancing)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Destination {
    /// Broadcast to all subscribers on topic (default)
    Broadcast,
    /// Unicast to specific actor instance
    Unicast(ActorId),
    /// Route to partition based on hash
    Partition(u64),
}

impl Default for Destination {
    fn default() -> Self {
        Self::Broadcast
    }
}

/// Unique identifier for an actor instance.
///
/// Used in Destination::Unicast to route messages to specific actors.
/// Actor IDs are typically generated randomly or assigned sequentially.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActorId(pub u64);

impl ActorId {
    /// Create a new random actor ID
    pub fn new() -> Self {
        Self(rand::random())
    }

    /// Create actor ID from a specific value
    pub fn from_u64(id: u64) -> Self {
        Self(id)
    }

    /// Get the underlying u64 value
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl Default for ActorId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ActorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

/// Correlation ID for request/reply pattern.
///
/// Used to match responses to requests in asynchronous request/reply flows.
/// Correlation IDs should be unique per request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CorrelationId(pub u128);

impl CorrelationId {
    /// Generate a new random correlation ID
    pub fn new() -> Self {
        Self(rand::random())
    }

    /// Create correlation ID from a specific value
    pub fn from_u128(id: u128) -> Self {
        Self(id)
    }

    /// Get the underlying u128 value
    pub fn as_u128(&self) -> u128 {
        self.0
    }
}

impl Default for CorrelationId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for CorrelationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

/// Trace ID for distributed tracing.
///
/// Used to track a message (or request) as it flows through the system.
/// All messages derived from the same user action should share the same TraceId.
///
/// ## Usage
///
/// ```rust
/// use mycelium_protocol::routing::TraceId;
///
/// // Generate new trace ID at system entry point
/// let trace_id = TraceId::new();
///
/// // Propagate trace_id through all derived messages
/// // envelope.trace_id = Some(trace_id);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TraceId(pub u128);

impl TraceId {
    /// Generate a new random trace ID
    pub fn new() -> Self {
        Self(rand::random())
    }

    /// Create trace ID from a specific value
    pub fn from_u128(id: u128) -> Self {
        Self(id)
    }

    /// Get the underlying u128 value
    pub fn as_u128(&self) -> u128 {
        self.0
    }
}

impl Default for TraceId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TraceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_destination_default() {
        assert_eq!(Destination::default(), Destination::Broadcast);
    }

    #[test]
    fn test_actor_id_creation() {
        let id1 = ActorId::new();
        let id2 = ActorId::new();
        // Random IDs should be different
        assert_ne!(id1, id2);

        let id3 = ActorId::from_u64(42);
        assert_eq!(id3.as_u64(), 42);
    }

    #[test]
    fn test_actor_id_display() {
        let id = ActorId::from_u64(0x123456789abcdef0);
        assert_eq!(format!("{}", id), "123456789abcdef0");
    }

    #[test]
    fn test_correlation_id_creation() {
        let id1 = CorrelationId::new();
        let id2 = CorrelationId::new();
        // Random IDs should be different
        assert_ne!(id1, id2);

        let id3 = CorrelationId::from_u128(42);
        assert_eq!(id3.as_u128(), 42);
    }

    #[test]
    fn test_correlation_id_display() {
        let id = CorrelationId::from_u128(0x123456789abcdef0);
        let formatted = format!("{}", id);
        assert_eq!(formatted, "0000000000000000123456789abcdef0");
        assert_eq!(formatted.len(), 32);
    }

    #[test]
    fn test_destination_variants() {
        let broadcast = Destination::Broadcast;
        let unicast = Destination::Unicast(ActorId::from_u64(1));
        let partition = Destination::Partition(42);

        assert_ne!(broadcast, unicast);
        assert_ne!(broadcast, partition);
        assert_ne!(unicast, partition);
    }

    #[test]
    fn test_trace_id_creation() {
        let id1 = TraceId::new();
        let id2 = TraceId::new();
        // Random IDs should be different
        assert_ne!(id1, id2);

        let id3 = TraceId::from_u128(42);
        assert_eq!(id3.as_u128(), 42);
    }

    #[test]
    fn test_trace_id_display() {
        let id = TraceId::from_u128(0x123456789abcdef0);
        let formatted = format!("{}", id);
        assert_eq!(formatted, "0000000000000000123456789abcdef0");
        assert_eq!(formatted.len(), 32);
    }
}
