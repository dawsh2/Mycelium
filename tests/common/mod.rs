//! Common test utilities and shared test infrastructure

use mycelium_protocol::impl_message;
use mycelium_transport::MessageBus;
use std::time::Duration;
use tokio::time::sleep;
use zerocopy::{AsBytes, FromBytes, FromZeroes};

// Common test message types
#[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
pub struct SwapEvent {
    pub amount_in: u128,
    pub amount_out: u128,
    pub pool_id: u64,
    pub timestamp: u64,
}

impl_message!(SwapEvent, 1, "market-data");

#[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
pub struct ArbitrageSignal {
    pub profit: u128,
    pub opportunity_id: u64,
    pub timestamp: u64,
}

impl_message!(ArbitrageSignal, 2, "arbitrage");

#[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
pub struct OrderExecution {
    pub order_id: u64,
    pub success: u8, // 1 = true, 0 = false (bool not safe for zerocopy)
    pub timestamp: u64,
}

impl_message!(OrderExecution, 3, "orders");

#[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
pub struct PoolStateUpdate {
    pub reserve0: u128,
    pub reserve1: u128,
    pub pool_address: u64,
    pub block_number: u64,
}

impl_message!(PoolStateUpdate, 4, "pool-state");

// Test utilities
pub fn create_test_swap_event(pool_id: u64) -> SwapEvent {
    SwapEvent {
        pool_id,
        amount_in: pool_id as u128 * 1000,
        amount_out: pool_id as u128 * 2000,
        timestamp: 123456789 + pool_id,
    }
}

pub fn create_test_arbitrage_signal(opportunity_id: u64) -> ArbitrageSignal {
    ArbitrageSignal {
        opportunity_id,
        profit: opportunity_id as u128 * 5000,
        timestamp: 987654321 + opportunity_id,
    }
}

pub fn create_test_order_execution(order_id: u64, success: bool) -> OrderExecution {
    OrderExecution {
        order_id,
        success: if success { 1 } else { 0 },
        timestamp: 111111111 + order_id,
    }
}

pub async fn wait_for_messages(ms: u64) {
    sleep(Duration::from_millis(ms)).await;
}

pub fn assert_message_received<T: PartialEq + std::fmt::Debug>(
    expected: T,
    actual: Option<T>,
    context: &str,
) {
    match actual {
        Some(received) => assert_eq!(
            received, expected,
            "Message mismatch in {}: expected {:?}, got {:?}",
            context, expected, received
        ),
        None => panic!("No message received in {}", context),
    }
}

// Performance testing utilities
pub struct PerformanceMetrics {
    pub message_count: usize,
    pub duration: Duration,
    pub throughput: f64, // messages per second
}

impl PerformanceMetrics {
    pub fn new(message_count: usize, duration: Duration) -> Self {
        let throughput = message_count as f64 / duration.as_secs_f64();
        Self {
            message_count,
            duration,
            throughput,
        }
    }

    pub fn assert_throughput_at_least(&self, min_throughput: f64, context: &str) {
        assert!(
            self.throughput >= min_throughput,
            "Throughput too low in {}: {:.2} msg/s (expected >= {:.2} msg/s)",
            context,
            self.throughput,
            min_throughput
        );
    }

    pub fn assert_latency_at_most(&self, max_latency_ms: u64, context: &str) {
        let avg_latency_ms = self.duration.as_millis() as u64 / self.message_count as u64;
        assert!(
            avg_latency_ms <= max_latency_ms,
            "Average latency too high in {}: {}ms (expected <= {}ms)",
            context,
            avg_latency_ms,
            max_latency_ms
        );
    }
}
