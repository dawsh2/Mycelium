//! Detailed breakdown of overhead in ServiceContext::emit()

use mycelium_protocol::impl_message;
use mycelium_transport::MessageBus;
use mycelium_transport::ServiceMetrics;
use std::sync::Arc;
use std::time::Instant;
use zerocopy::{AsBytes, FromBytes, FromZeroes};

#[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
struct TestMsg {
    value: u64,
}

impl_message!(TestMsg, 1, "test.msg");

fn measure<F: Fn()>(name: &str, iterations: u128, f: F) -> u128 {
    // Warmup
    for _ in 0..1000 {
        f();
    }

    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let total_ns = start.elapsed().as_nanos();
    let avg_ns = total_ns / iterations;

    println!("{:50} {} ns", name, avg_ns);
    avg_ns
}

async fn measure_async<F, Fut>(name: &str, iterations: u128, f: F) -> u128
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    // Warmup
    for _ in 0..1000 {
        f().await;
    }

    let start = Instant::now();
    for _ in 0..iterations {
        f().await;
    }
    let total_ns = start.elapsed().as_nanos();
    let avg_ns = total_ns / iterations;

    println!("{:50} {} ns", name, avg_ns);
    avg_ns
}

#[tokio::main]
async fn main() {
    println!("=== Overhead Breakdown Analysis ===\n");

    let iterations = 100_000;
    let bus = MessageBus::new();
    let _sub = bus.subscriber::<TestMsg>();
    let metrics = ServiceMetrics::new();
    let trace_id = "test_trace_id".to_string();

    println!("Baseline operations:");
    println!("{}", "=".repeat(80));

    // 1. Baseline - empty function
    measure("1. Empty function call", iterations, || {});

    // 2. Instant::now() overhead
    measure("2. Instant::now() (1 call)", iterations, || {
        let _start = Instant::now();
    });

    measure("3. Instant::now() + elapsed (2 calls)", iterations, || {
        let start = Instant::now();
        let _elapsed = start.elapsed();
    });

    // 3. Tracing overhead
    measure("4. tracing::debug! (disabled)", iterations, || {
        tracing::debug!(trace_id = %trace_id, "test message");
    });

    // 4. Metrics recording
    measure("5. metrics.record_emit()", iterations, || {
        metrics.record_emit(100);
    });

    // 5. Publisher creation (DashMap lookup)
    measure("6. bus.publisher() (DashMap lookup)", iterations, || {
        let _pub = bus.publisher::<TestMsg>();
    });

    // 6. Arc clone
    let bus_arc = Arc::new(bus.clone());
    measure("7. Arc::clone()", iterations, || {
        let _clone = Arc::clone(&bus_arc);
    });

    println!("\n{}", "=".repeat(80));
    println!("Combined operations:");
    println!("{}", "=".repeat(80));

    // Measure actual publish
    let pub_ = bus.publisher::<TestMsg>();
    let baseline = measure_async("8. Raw publish (cached publisher)", iterations, || async {
        pub_.publish(TestMsg { value: 42 }).await.unwrap();
    })
    .await;

    // Measure with timing
    let with_timing = measure_async("9. Publish + timing", iterations, || async {
        let start = Instant::now();
        pub_.publish(TestMsg { value: 42 }).await.unwrap();
        let _elapsed = start.elapsed();
    })
    .await;

    // Measure with timing + metrics
    let with_metrics = measure_async("10. Publish + timing + metrics", iterations, || async {
        let start = Instant::now();
        pub_.publish(TestMsg { value: 42 }).await.unwrap();
        let latency_us = start.elapsed().as_micros() as u64;
        metrics.record_emit(latency_us);
    })
    .await;

    // Measure with publisher lookup
    let with_lookup = measure_async("11. Publish + publisher lookup", iterations, || async {
        let pub_ = bus.publisher::<TestMsg>();
        pub_.publish(TestMsg { value: 42 }).await.unwrap();
    })
    .await;

    // Measure everything (simulating ctx.emit but without tracing)
    let full_no_trace = measure_async("12. Full ctx.emit() (no tracing)", iterations, || async {
        let start = Instant::now();
        let pub_ = bus.publisher::<TestMsg>();
        pub_.publish(TestMsg { value: 42 }).await.unwrap();
        let latency_us = start.elapsed().as_micros() as u64;
        metrics.record_emit(latency_us);
    })
    .await;

    // Measure with tracing
    let full_with_trace = measure_async("13. Full ctx.emit() (with tracing)", iterations, || async {
        let start = Instant::now();
        tracing::debug!(trace_id = %trace_id, type_id = 1, topic = "test.msg", "Emitting message");
        let pub_ = bus.publisher::<TestMsg>();
        pub_.publish(TestMsg { value: 42 }).await.unwrap();
        let latency_us = start.elapsed().as_micros() as u64;
        metrics.record_emit(latency_us);
        tracing::trace!(trace_id = %trace_id, topic = "test.msg", latency_us = latency_us, "Message emitted");
    }).await;

    println!("\n{}", "=".repeat(80));
    println!("Overhead breakdown:");
    println!("{}", "=".repeat(80));
    println!("Baseline publish:                     {} ns", baseline);
    println!(
        "+ Timing overhead:                    +{} ns",
        with_timing.saturating_sub(baseline)
    );
    println!(
        "+ Metrics overhead:                   +{} ns",
        with_metrics.saturating_sub(with_timing)
    );
    println!(
        "+ Publisher lookup:                   +{} ns",
        with_lookup.saturating_sub(baseline)
    );
    println!(
        "Full (no tracing):                    {} ns ({}x slower)",
        full_no_trace,
        full_no_trace / baseline.max(1)
    );
    println!(
        "Full (with tracing):                  {} ns ({}x slower)",
        full_with_trace,
        full_with_trace / baseline.max(1)
    );
    println!(
        "\nTotal overhead from ServiceContext:   {} ns",
        full_with_trace.saturating_sub(baseline)
    );
}
