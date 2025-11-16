//! Topic naming conventions and helpers
//!
//! This module provides consistent topic naming patterns for different routing scenarios:
//! - Broadcast topics: Standard pub/sub (e.g., "orderbook.updates")
//! - Actor mailboxes: Unicast routing (e.g., "actor.123abc")
//! - Partitioned topics: Load balancing (e.g., "orderbook.0", "orderbook.1")
//!
//! These helpers ensure consistent naming across the system and make actor
//! routing patterns explicit.

use mycelium_protocol::routing::ActorId;

/// Helper for building topic names with consistent conventions
pub struct TopicBuilder;

impl TopicBuilder {
    /// Create a broadcast topic name (standard pub/sub)
    ///
    /// Use this for messages that should reach all subscribers.
    ///
    /// # Example
    /// ```
    /// use crate::TopicBuilder;
    ///
    /// let topic = TopicBuilder::broadcast("orderbook.updates");
    /// assert_eq!(topic, "orderbook.updates");
    /// ```
    pub fn broadcast(name: &str) -> String {
        name.to_string()
    }

    /// Create an actor mailbox topic (unicast routing)
    ///
    /// Actor mailboxes are just pub/sub topics with a single subscriber.
    /// The naming convention "actor.<id>" makes it clear this is a mailbox.
    ///
    /// # Example
    /// ```
    /// use crate::TopicBuilder;
    /// use mycelium_protocol::routing::ActorId;
    ///
    /// let actor_id = ActorId::from_u64(0x123abc);
    /// let topic = TopicBuilder::actor_mailbox(actor_id);
    /// assert_eq!(topic, "actor.0000000000123abc");
    /// ```
    pub fn actor_mailbox(actor_id: ActorId) -> String {
        format!("actor.{}", actor_id)
    }

    /// Create a partitioned topic name (load balancing)
    ///
    /// Use this for horizontally scaled services where messages should be
    /// distributed across multiple instances based on partition key.
    ///
    /// # Example
    /// ```
    /// use crate::TopicBuilder;
    ///
    /// let topic = TopicBuilder::partition("orderbook", 0);
    /// assert_eq!(topic, "orderbook.0");
    /// ```
    pub fn partition(base: &str, partition: usize) -> String {
        format!("{}.{}", base, partition)
    }

    /// Create a partitioned topic name with hash-based routing hint
    ///
    /// Use this when you have a hash value and want to route to a specific
    /// partition out of N total partitions.
    ///
    /// # Example
    /// ```
    /// use crate::TopicBuilder;
    ///
    /// let hash = 12345u64;
    /// let topic = TopicBuilder::partition_by_hash("orderbook", hash, 4);
    /// assert_eq!(topic, "orderbook.1"); // 12345 % 4 = 1
    /// ```
    pub fn partition_by_hash(base: &str, hash: u64, num_partitions: usize) -> String {
        let partition = (hash as usize) % num_partitions.max(1);
        Self::partition(base, partition)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_broadcast() {
        assert_eq!(TopicBuilder::broadcast("test"), "test");
        assert_eq!(
            TopicBuilder::broadcast("orderbook.updates"),
            "orderbook.updates"
        );
    }

    #[test]
    fn test_actor_mailbox() {
        let actor_id = ActorId::from_u64(0x123abc);
        let topic = TopicBuilder::actor_mailbox(actor_id);
        assert_eq!(topic, "actor.0000000000123abc");
    }

    #[test]
    fn test_partition() {
        assert_eq!(TopicBuilder::partition("orderbook", 0), "orderbook.0");
        assert_eq!(TopicBuilder::partition("orderbook", 42), "orderbook.42");
    }

    #[test]
    fn test_partition_by_hash() {
        // Hash 12345 % 4 = 1
        assert_eq!(
            TopicBuilder::partition_by_hash("orderbook", 12345, 4),
            "orderbook.1"
        );

        // Hash 100 % 3 = 1
        assert_eq!(TopicBuilder::partition_by_hash("test", 100, 3), "test.1");

        // Edge case: 0 partitions (should default to 1)
        assert_eq!(TopicBuilder::partition_by_hash("test", 100, 0), "test.0");
    }
}
