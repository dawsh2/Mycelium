//! Test ONLY ctx.emit() without any other logging

use anyhow::Result;
use mycelium_protocol::impl_message;
use mycelium_transport::MessageBus;
use mycelium_transport::{service, ServiceContext, ServiceRuntime};
use zerocopy::{AsBytes, FromBytes, FromZeroes};

#[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
struct TestEvent {
    value: u64,
}

impl_message!(TestEvent, 100, "test.events");

struct EmitOnlyService {
    count: u64,
    max_count: u64,
}

impl EmitOnlyService {
    fn new(max_count: u64) -> Self {
        Self {
            count: 0,
            max_count,
        }
    }
}

#[service]
impl EmitOnlyService {
    async fn run(&mut self, ctx: &ServiceContext) -> Result<()> {
        while self.count < self.max_count {
            // ONLY emit - no logging, no formatting, nothing else
            ctx.emit(TestEvent { value: self.count }).await?;
            self.count += 1;
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Minimal logging setup
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN) // Only warnings and errors
        .init();

    let service = EmitOnlyService::new(10_000);

    let bus = MessageBus::new();
    let _sub = bus.subscriber::<TestEvent>();

    let runtime = ServiceRuntime::new(bus);
    let handle = runtime.spawn_service(service).await?;

    handle.join().await?;

    // Metrics will show the actual emit latency
    Ok(())
}
