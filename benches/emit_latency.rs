//! Benchmark to measure actual emit() latency and identify bottlenecks

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use mycelium_protocol::impl_message;
use mycelium_transport::{MessageBus, Publisher};
use zerocopy::{IntoBytes, FromBytes, FromZeros, Immutable};

#[derive(Debug, Clone, Copy, PartialEq, IntoBytes, FromBytes, FromZeros, Immutable)]
#[repr(C)]
struct BenchMsg {
    value: u64,
}

impl_message!(BenchMsg, 1, "bench.msg");

fn bench_publisher_creation(c: &mut Criterion) {
    let bus = MessageBus::new();

    c.bench_function("publisher_creation", |b| {
        b.iter(|| {
            let _pub = bus.publisher::<BenchMsg>();
        });
    });
}

fn bench_direct_publish(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let bus = MessageBus::new();

    // Create a subscriber so publish doesn't fail
    let _sub = bus.subscriber::<BenchMsg>();

    c.bench_function("direct_publish_with_creation", |b| {
        b.to_async(&rt).iter(|| async {
            let pub_ = bus.publisher::<BenchMsg>();
            pub_.publish(black_box(BenchMsg { value: 42 }))
                .await
                .unwrap();
        });
    });
}

fn bench_cached_publisher(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let bus = MessageBus::new();

    // Create a subscriber so publish doesn't fail
    let _sub = bus.subscriber::<BenchMsg>();

    // Pre-create publisher (cached)
    let pub_ = bus.publisher::<BenchMsg>();

    c.bench_function("cached_publisher_publish", |b| {
        b.to_async(&rt).iter(|| async {
            pub_.publish(black_box(BenchMsg { value: 42 }))
                .await
                .unwrap();
        });
    });
}

fn bench_try_publish_sync(c: &mut Criterion) {
    let bus = MessageBus::new();

    // Create a subscriber so publish doesn't fail
    let _sub = bus.subscriber::<BenchMsg>();

    // Pre-create publisher
    let pub_ = bus.publisher::<BenchMsg>();

    c.bench_function("try_publish_sync", |b| {
        b.iter(|| {
            pub_.try_publish(black_box(BenchMsg { value: 42 })).unwrap();
        });
    });
}

fn bench_raw_broadcast_send(c: &mut Criterion) {
    use mycelium_protocol::Envelope;
    use tokio::sync::broadcast;

    let (tx, _rx) = broadcast::channel::<Envelope>(1000);
    let msg = BenchMsg { value: 42 };

    c.bench_function("raw_broadcast_send", |b| {
        b.iter(|| {
            let envelope = Envelope::new(black_box(msg));
            tx.send(envelope).unwrap();
        });
    });
}

fn bench_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("emit_comparison");
    let rt = tokio::runtime::Runtime::new().unwrap();

    let bus = MessageBus::new();
    let _sub = bus.subscriber::<BenchMsg>();
    let pub_ = bus.publisher::<BenchMsg>();

    group.bench_function("uncached", |b| {
        b.to_async(&rt).iter(|| async {
            let pub_ = bus.publisher::<BenchMsg>();
            pub_.publish(black_box(BenchMsg { value: 42 }))
                .await
                .unwrap();
        });
    });

    group.bench_function("cached", |b| {
        b.to_async(&rt).iter(|| async {
            pub_.publish(black_box(BenchMsg { value: 42 }))
                .await
                .unwrap();
        });
    });

    group.bench_function("sync", |b| {
        b.iter(|| {
            pub_.try_publish(black_box(BenchMsg { value: 42 })).unwrap();
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_publisher_creation,
    bench_direct_publish,
    bench_cached_publisher,
    bench_try_publish_sync,
    bench_raw_broadcast_send,
    bench_comparison
);
criterion_main!(benches);
