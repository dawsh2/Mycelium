//! Quick latency measurement for emit operations

use mycelium_protocol::impl_message;
use mycelium_transport::MessageBus;
use std::time::Instant;
use zerocopy::{AsBytes, FromBytes, FromZeroes};

#[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
struct TestMsg {
    value: u64,
}

impl_message!(TestMsg, 1, "test.msg");

#[tokio::main]
async fn main() {
    println!("=== Emit Latency Measurements ===\n");

    let bus = MessageBus::new();
    let _sub = bus.subscriber::<TestMsg>();

    // Warmup
    for _ in 0..1000 {
        let pub_ = bus.publisher::<TestMsg>();
        pub_.publish(TestMsg { value: 42 }).await.unwrap();
    }

    println!("1. Publisher creation + publish (uncached):");
    let mut total = 0u128;
    let iterations = 10_000;
    for _ in 0..iterations {
        let start = Instant::now();
        let pub_ = bus.publisher::<TestMsg>();
        pub_.publish(TestMsg { value: 42 }).await.unwrap();
        total += start.elapsed().as_nanos();
    }
    println!("   Average: {} ns", total / iterations);

    println!("\n2. Publish with cached publisher:");
    let pub_ = bus.publisher::<TestMsg>();
    total = 0;
    for _ in 0..iterations {
        let start = Instant::now();
        pub_.publish(TestMsg { value: 42 }).await.unwrap();
        total += start.elapsed().as_nanos();
    }
    println!("   Average: {} ns", total / iterations);

    println!("\n3. Sync publish (try_publish) with cached publisher:");
    total = 0;
    for _ in 0..iterations {
        let start = Instant::now();
        pub_.try_publish(TestMsg { value: 42 }).unwrap();
        total += start.elapsed().as_nanos();
    }
    println!("   Average: {} ns", total / iterations);

    println!("\n4. Just publisher creation:");
    total = 0;
    for _ in 0..iterations {
        let start = Instant::now();
        let _pub = bus.publisher::<TestMsg>();
        total += start.elapsed().as_nanos();
    }
    println!("   Average: {} ns", total / iterations);

    println!("\n5. Raw broadcast send:");
    use mycelium_protocol::Envelope;
    use tokio::sync::broadcast;
    let (tx, _rx) = broadcast::channel::<Envelope>(1000);
    total = 0;
    let msg = TestMsg { value: 42 };
    for _ in 0..iterations {
        let start = Instant::now();
        let envelope = Envelope::new(msg);
        tx.send(envelope).unwrap();
        total += start.elapsed().as_nanos();
    }
    println!("   Average: {} ns", total / iterations);
}
