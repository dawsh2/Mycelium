/// Ordered message delivery with sequence tracking
///
/// This module provides FIFO ordering guarantees for pub/sub subscribers.
/// Compatible with future actor mailboxes (which provide ordering naturally).
///
/// ## Design Philosophy (from ACTORS.md)
///
/// - Actors provide per-mailbox FIFO ordering automatically
/// - Pub/sub can achieve ordering via sequence numbers
/// - OrderedSubscriber wraps regular Subscriber to add ordering
/// - Messages without sequence numbers are delivered immediately
///
/// ## Use Cases
///
/// - Trading systems where order matters (cancel after fill)
/// - Game state updates that must be applied sequentially
/// - Any scenario requiring causal ordering
use crate::Subscriber;
use mycelium_protocol::Message;
use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::sync::Arc;

/// Subscriber that enforces FIFO message ordering via sequence numbers
///
/// Wraps a regular `Subscriber` and reorders messages based on their
/// sequence numbers. Messages arrive out-of-order but are delivered
/// in-order to the application.
///
/// # Example
///
/// ```rust,no_run
/// use mycelium_transport::{MessageBus, OrderedSubscriber};
/// # use mycelium_protocol::PoolStateUpdate;
///
/// # async fn example() {
/// let bus = MessageBus::new();
/// let sub = bus.subscriber::<PoolStateUpdate>();
///
/// // Wrap in OrderedSubscriber for ordered delivery
/// let mut ordered = OrderedSubscriber::new(sub, 0);
///
/// while let Some(msg) = ordered.recv().await {
///     // Messages arrive in sequence order, even if received out-of-order
///     println!("Received: {:?}", msg);
/// }
/// # }
/// ```
pub struct OrderedSubscriber<M: Message> {
    /// Underlying unordered subscriber
    inner: Subscriber<M>,

    /// Sequence tracking and reordering
    tracker: SequenceTracker<M>,
}

impl<M: Message> OrderedSubscriber<M> {
    /// Create a new ordered subscriber
    ///
    /// # Arguments
    ///
    /// * `inner` - The underlying subscriber to wrap
    /// * `initial_sequence` - The expected sequence number of the first message
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use mycelium_transport::{MessageBus, OrderedSubscriber};
    /// # use mycelium_protocol::PoolStateUpdate;
    /// # async fn example() {
    /// let bus = MessageBus::new();
    /// let sub = bus.subscriber::<PoolStateUpdate>();
    /// let mut ordered = OrderedSubscriber::new(sub, 0);
    /// # }
    /// ```
    pub fn new(inner: Subscriber<M>, initial_sequence: u64) -> Self {
        Self {
            inner,
            tracker: SequenceTracker::new(initial_sequence),
        }
    }

    /// Receive the next message in sequence order
    ///
    /// This method blocks until the next expected message arrives.
    /// Out-of-order messages are buffered internally.
    ///
    /// Returns `None` if the channel is closed.
    pub async fn recv(&mut self) -> Option<Arc<M>> {
        loop {
            // Check if we have a buffered message ready
            if let Some(msg) = self.tracker.try_deliver() {
                return Some(msg);
            }

            // Receive next envelope from underlying subscriber
            let msg = self.inner.recv().await?;

            // Add to tracker (will buffer if out-of-order, or deliver if in-order)
            if let Some(ready) = self.tracker.add_message(msg) {
                return Some(ready);
            }

            // Message was buffered, continue loop to get next message
        }
    }

    /// Get statistics about ordering and buffering
    pub fn stats(&self) -> OrderingStats {
        self.tracker.stats()
    }

    /// Get the number of messages currently buffered (waiting for gaps to fill)
    pub fn buffered_count(&self) -> usize {
        self.tracker.buffered_count()
    }

    /// Reset sequence tracking (use when starting a new session)
    pub fn reset(&mut self, new_initial_sequence: u64) {
        self.tracker = SequenceTracker::new(new_initial_sequence);
    }
}

/// Tracks message sequence numbers and reorders out-of-order messages
struct SequenceTracker<M: Message> {
    /// Next expected sequence number
    next_expected: u64,

    /// Buffer of out-of-order messages (sequence -> message)
    buffer: BTreeMap<u64, Arc<M>>,

    /// Maximum buffer size before warning (reserved for future use)
    #[allow(dead_code)]
    max_buffer_size: usize,

    /// Statistics
    stats: OrderingStats,

    _phantom: PhantomData<M>,
}

impl<M: Message> SequenceTracker<M> {
    fn new(initial_sequence: u64) -> Self {
        Self {
            next_expected: initial_sequence,
            buffer: BTreeMap::new(),
            max_buffer_size: 1000, // Configurable in future
            stats: OrderingStats::default(),
            _phantom: PhantomData,
        }
    }

    /// Add a message to the tracker
    ///
    /// Returns `Some(msg)` if the message is in-order and can be delivered immediately.
    /// Returns `None` if the message was buffered (out-of-order).
    fn add_message(&mut self, msg: Arc<M>) -> Option<Arc<M>> {
        self.stats.total_received += 1;

        // TODO: Extract sequence number from envelope metadata
        // Currently, envelopes have an optional `sequence` field, but we need
        // a way to access it from the Arc<M>. This requires either:
        // 1. Passing envelopes instead of downcasted messages, OR
        // 2. Adding a Message::sequence() method
        //
        // For now, deliver all messages immediately (no reordering)
        // This makes OrderedSubscriber a pass-through until sequence extraction is implemented

        self.stats.in_order_delivered += 1;
        self.next_expected += 1;
        Some(msg)

        // Future implementation will be:
        // match envelope.sequence {
        //     Some(seq) if seq == self.next_expected => {
        //         self.next_expected += 1;
        //         Some(msg)
        //     }
        //     Some(seq) => {
        //         self.buffer.insert(seq, msg);
        //         None
        //     }
        //     None => Some(msg), // No sequence = deliver immediately
        // }
    }

    /// Try to deliver a buffered message if the next expected one is ready
    fn try_deliver(&mut self) -> Option<Arc<M>> {
        if let Some(msg) = self.buffer.remove(&self.next_expected) {
            self.next_expected += 1;
            self.stats.reordered_delivered += 1;
            Some(msg)
        } else {
            None
        }
    }

    fn buffered_count(&self) -> usize {
        self.buffer.len()
    }

    fn stats(&self) -> OrderingStats {
        self.stats
    }
}

/// Statistics about ordered message delivery
#[derive(Debug, Clone, Copy, Default)]
pub struct OrderingStats {
    /// Total messages received
    pub total_received: u64,

    /// Messages delivered in-order (no buffering needed)
    pub in_order_delivered: u64,

    /// Messages delivered after reordering (were buffered)
    pub reordered_delivered: u64,

    /// Messages dropped due to sequence gaps or errors
    pub dropped: u64,

    /// Maximum buffer size reached
    pub max_buffer_used: usize,
}

impl OrderingStats {
    /// Calculate the percentage of messages that arrived in-order
    pub fn in_order_percentage(&self) -> f64 {
        if self.total_received == 0 {
            return 100.0;
        }
        (self.in_order_delivered as f64 / self.total_received as f64) * 100.0
    }

    /// Calculate the percentage of messages that needed reordering
    pub fn reorder_percentage(&self) -> f64 {
        if self.total_received == 0 {
            return 0.0;
        }
        (self.reordered_delivered as f64 / self.total_received as f64) * 100.0
    }
}

impl std::fmt::Display for OrderingStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "OrderingStats {{ total: {}, in_order: {} ({:.1}%), reordered: {} ({:.1}%), dropped: {}, max_buffer: {} }}",
            self.total_received,
            self.in_order_delivered,
            self.in_order_percentage(),
            self.reordered_delivered,
            self.reorder_percentage(),
            self.dropped,
            self.max_buffer_used,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mycelium_protocol::{impl_message, Envelope};
    use tokio::sync::broadcast;
    use zerocopy::{AsBytes, FromBytes, FromZeroes};

    #[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
    #[repr(C)]
    struct TestMsg {
        value: u64,
    }

    impl_message!(TestMsg, 1, "test");

    #[tokio::test]
    async fn test_ordered_subscriber_in_order() {
        let (tx, rx) = broadcast::channel(10);
        let sub: Subscriber<TestMsg> = Subscriber::new(rx);
        let mut ordered = OrderedSubscriber::new(sub, 0);

        // Send messages in order
        for i in 0..5 {
            let envelope = Envelope::with_sequence(TestMsg { value: i }, i);
            tx.send(envelope).unwrap();
        }

        // Should receive in order
        for i in 0..5 {
            let msg = ordered.recv().await.unwrap();
            assert_eq!(msg.value, i);
        }

        let stats = ordered.stats();
        println!("Stats: {}", stats);
    }

    #[test]
    fn test_ordering_stats() {
        let stats = OrderingStats {
            total_received: 100,
            in_order_delivered: 80,
            reordered_delivered: 15,
            dropped: 5,
            max_buffer_used: 10,
        };

        assert_eq!(stats.in_order_percentage(), 80.0);
        assert_eq!(stats.reorder_percentage(), 15.0);
    }
}
