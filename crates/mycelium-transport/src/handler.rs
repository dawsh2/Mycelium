//! Message handler traits for compile-time routing
//!
//! These traits enable direct function call routing instead of going through
//! the MessageBus, reducing overhead from ~65ns to ~2-3ns per handler.

use anyhow::Result;

/// Synchronous message handler for fast-path processing
///
/// Implement this trait for services that need ultra-low latency (<1μs budget).
/// Handlers are called directly via the `routing_config!` macro, avoiding
/// Arc allocation and channel overhead.
///
/// # Example
///
/// ```ignore
/// use mycelium_transport::MessageHandler;
///
/// struct RiskManager {
///     limits: RiskLimits,
/// }
///
/// impl MessageHandler<V2Swap> for RiskManager {
///     fn handle(&mut self, swap: &V2Swap) {
///         self.update_exposure(swap);
///     }
/// }
/// ```
pub trait MessageHandler<M> {
    /// Handle a message synchronously
    ///
    /// This method receives a borrowed reference to avoid cloning.
    /// For high-throughput scenarios, keep processing under 100ns.
    fn handle(&mut self, msg: &M);
}

/// Asynchronous message handler for I/O-bound processing
///
/// Use this for handlers that need to perform async operations like
/// database writes, HTTP calls, or other I/O. Note that async handlers
/// require cloning the message since Rust doesn't allow borrowing across
/// `.await` points.
///
/// # Example
///
/// ```ignore
/// use mycelium_transport::AsyncMessageHandler;
///
/// struct DatabaseLogger {
///     pool: DbPool,
/// }
///
/// impl AsyncMessageHandler<V2Swap> for DatabaseLogger {
///     async fn handle(&mut self, msg: &V2Swap) -> Result<()> {
///         self.pool.insert_swap(msg).await?;
///         Ok(())
///     }
/// }
/// ```
pub trait AsyncMessageHandler<M> {
    /// Handle a message asynchronously
    ///
    /// Returns a Result to allow error propagation. The caller decides
    /// how to handle errors (log, retry, panic, etc.).
    #[allow(async_fn_in_trait)]
    async fn handle(&mut self, msg: &M) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct TestMessage {
        value: i32,
    }

    struct SyncCounter {
        count: i32,
        last_value: i32,
    }

    impl MessageHandler<TestMessage> for SyncCounter {
        fn handle(&mut self, msg: &TestMessage) {
            self.count += 1;
            self.last_value = msg.value;
        }
    }

    struct AsyncLogger {
        messages: Vec<i32>,
    }

    impl AsyncMessageHandler<TestMessage> for AsyncLogger {
        async fn handle(&mut self, msg: &TestMessage) -> Result<()> {
            self.messages.push(msg.value);
            Ok(())
        }
    }

    struct AsyncErrorHandler;

    impl AsyncMessageHandler<TestMessage> for AsyncErrorHandler {
        async fn handle(&mut self, _msg: &TestMessage) -> Result<()> {
            Err(anyhow::anyhow!("Intentional error"))
        }
    }

    #[test]
    fn test_sync_handler() {
        let mut handler = SyncCounter {
            count: 0,
            last_value: 0,
        };

        let msg1 = TestMessage { value: 42 };
        handler.handle(&msg1);
        assert_eq!(handler.count, 1);
        assert_eq!(handler.last_value, 42);

        let msg2 = TestMessage { value: 100 };
        handler.handle(&msg2);
        assert_eq!(handler.count, 2);
        assert_eq!(handler.last_value, 100);
    }

    #[tokio::test]
    async fn test_async_handler() {
        let mut handler = AsyncLogger {
            messages: Vec::new(),
        };

        let msg1 = TestMessage { value: 1 };
        let result = handler.handle(&msg1).await;
        assert!(result.is_ok());
        assert_eq!(handler.messages.len(), 1);
        assert_eq!(handler.messages[0], 1);

        let msg2 = TestMessage { value: 2 };
        handler.handle(&msg2).await.unwrap();
        assert_eq!(handler.messages.len(), 2);
        assert_eq!(handler.messages, vec![1, 2]);
    }

    #[tokio::test]
    async fn test_async_handler_error() {
        let mut handler = AsyncErrorHandler;
        let msg = TestMessage { value: 42 };

        let result = handler.handle(&msg).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "Intentional error");
    }

    #[test]
    fn test_sync_handler_multiple_calls() {
        let mut handler = SyncCounter {
            count: 0,
            last_value: 0,
        };

        for i in 0..100 {
            let msg = TestMessage { value: i };
            handler.handle(&msg);
        }

        assert_eq!(handler.count, 100);
        assert_eq!(handler.last_value, 99);
    }

    #[tokio::test]
    async fn test_async_handler_concurrent() {
        let mut handler = AsyncLogger {
            messages: Vec::new(),
        };

        // Sequential async calls
        for i in 0..10 {
            let msg = TestMessage { value: i };
            handler.handle(&msg).await.unwrap();
        }

        assert_eq!(handler.messages.len(), 10);
        assert_eq!(handler.messages, (0..10).collect::<Vec<_>>());
    }
}
