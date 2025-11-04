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
    async fn handle(&mut self, msg: &M) -> Result<()>;
}
