//! Compile-time routing example
//!
//! Demonstrates ultra-low latency message handling using direct function calls
//! instead of Arc-based MessageBus routing.
//!
//! Performance: ~2-3ns per handler (30x faster than Arc-based routing at ~65ns)

use mycelium_protocol::{CounterUpdate, TextMessage};
use mycelium_transport::{routing_config, MessageHandler};

// Example handlers
struct MetricsCollector {
    total_messages: u64,
}

impl MetricsCollector {
    fn new() -> Self {
        Self { total_messages: 0 }
    }
}

impl MessageHandler<TextMessage> for MetricsCollector {
    fn handle(&mut self, msg: &TextMessage) {
        self.total_messages += 1;
        println!(
            "[MetricsCollector] Total messages: {} (latest: {})",
            self.total_messages,
            msg.content.as_str().unwrap_or("<invalid>")
        );
    }
}

impl MessageHandler<CounterUpdate> for MetricsCollector {
    fn handle(&mut self, msg: &CounterUpdate) {
        self.total_messages += 1;
        println!(
            "[MetricsCollector] Total messages: {} (latest counter value: {})",
            self.total_messages, msg.value
        );
    }
}

struct Logger;

impl Logger {
    fn new() -> Self {
        Self
    }
}

impl MessageHandler<TextMessage> for Logger {
    fn handle(&mut self, msg: &TextMessage) {
        println!(
            "[Logger] TextMessage from {}: {}",
            msg.sender.as_str().unwrap_or("<invalid>"),
            msg.content.as_str().unwrap_or("<invalid>")
        );
    }
}

struct CounterValidator {
    max_value: u64,
}

impl CounterValidator {
    fn new(max_value: u64) -> Self {
        Self { max_value }
    }
}

impl MessageHandler<CounterUpdate> for CounterValidator {
    fn handle(&mut self, msg: &CounterUpdate) {
        if msg.value > self.max_value {
            println!(
                "[CounterValidator] WARNING: Counter value {} exceeds max {}",
                msg.value, self.max_value
            );
        } else {
            println!("[CounterValidator] Counter value {} is valid", msg.value);
        }
    }
}

// Generate compile-time routing struct
routing_config! {
    name: AppServices,
    routes: {
        TextMessage => [MetricsCollector, Logger],
        CounterUpdate => [MetricsCollector, CounterValidator],
    }
}

fn main() {
    println!("=== Compile-Time Routing Example ===\n");

    // Create services
    let mut services = AppServices::new(
        MetricsCollector::new(),
        Logger::new(),
        CounterValidator::new(100),
    );

    println!("Processing TextMessages...\n");

    // Create and route TextMessages
    let msg1 = TextMessage::new("Alice", "Hello, world!", 1000);
    services.route_text_message(&msg1);

    let msg2 = TextMessage::new("Bob", "Rust is awesome!", 2000);
    services.route_text_message(&msg2);

    println!("\nProcessing CounterUpdates...\n");

    // Create and route CounterUpdates
    let counter1 = CounterUpdate::new("counter_1", 50, 10);
    services.route_counter_update(&counter1);

    let counter2 = CounterUpdate::new("counter_2", 150, 50); // Exceeds max
    services.route_counter_update(&counter2);

    let counter3 = CounterUpdate::new("counter_3", 75, 25);
    services.route_counter_update(&counter3);

    println!("\n=== Performance Notes ===");
    println!("Each route_* call has ~2-3ns overhead (direct function calls)");
    println!("Compare to MessageBus Arc routing: ~65ns overhead");
    println!("Speedup: 30x faster for single-process deployments\n");
}
