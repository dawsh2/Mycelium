/// Generic sensor network example - domain-agnostic pub/sub
///
/// Demonstrates Mycelium for IoT/sensor data collection without DeFi terminology.
/// This example works for any sensor network: temperature, motion, industrial IoT, etc.
///
/// Run with: cargo run --example sensor_network
use mycelium_protocol::{impl_message, Message};
use mycelium_transport::MessageBus;
use zerocopy::{AsBytes, FromBytes, FromZeroes};

/// Generic sensor reading - works for any sensor type
#[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
struct SensorReading {
    sensor_id: u64,
    value: i64, // Raw sensor value (scaled to i64)
    timestamp_ms: u64,
    quality: u64, // 0-100 quality indicator (u64 to avoid padding)
}

impl_message!(SensorReading, 1, "sensors");

/// Alert message when sensor exceeds threshold
#[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
#[repr(C)]
struct SensorAlert {
    sensor_id: u64,
    threshold_exceeded: i64,
    current_value: i64,
    severity: u64, // 1=low, 2=medium, 3=high, 4=critical (u64 to avoid padding)
}

impl_message!(SensorAlert, 2, "alerts");

#[tokio::main]
async fn main() {
    println!("=== Mycelium: Generic Sensor Network Example ===\n");

    let bus = MessageBus::new();

    // Data collector publishes sensor readings
    let sensor_pub = bus.publisher::<SensorReading>();

    // Alert system publishes alerts
    let alert_pub = bus.publisher::<SensorAlert>();

    // Multiple consumers subscribe
    let mut data_logger = bus.subscriber::<SensorReading>();
    let mut alerting_system = bus.subscriber::<SensorReading>();
    let mut alert_receiver = bus.subscriber::<SensorAlert>();

    println!("📡 Sensor network initialized");
    println!("   - 1 data collector");
    println!("   - 2 reading subscribers (logger, alerting)");
    println!("   - 1 alert subscriber\n");

    // Spawn data logger
    tokio::spawn(async move {
        while let Some(reading) = data_logger.recv().await {
            println!(
                "📝 [Logger] Sensor {}: value={}, quality={}%",
                reading.sensor_id, reading.value, reading.quality
            );
        }
    });

    // Spawn alerting system (monitors readings and publishes alerts)
    tokio::spawn(async move {
        while let Some(reading) = alerting_system.recv().await {
            // Check threshold (example: alert if value > 1000)
            if reading.value > 1000 {
                let alert = SensorAlert {
                    sensor_id: reading.sensor_id,
                    threshold_exceeded: 1000,
                    current_value: reading.value,
                    severity: if reading.value > 2000 { 4 } else { 2 },
                };

                alert_pub.publish(alert).await.ok();
            }
        }
    });

    // Spawn alert receiver
    tokio::spawn(async move {
        while let Some(alert) = alert_receiver.recv().await {
            let severity_str = match alert.severity {
                1 => "LOW",
                2 => "MEDIUM",
                3 => "HIGH",
                4 => "CRITICAL",
                _ => "UNKNOWN",
            };
            println!(
                "🚨 [Alert] Sensor {} ALARM: {} (threshold: {}) - Severity: {}",
                alert.sensor_id, alert.current_value, alert.threshold_exceeded, severity_str
            );
        }
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Simulate sensor data collection
    println!("🔄 Collecting sensor data...\n");

    let sensor_data = vec![
        (101, 450, 95),  // Normal reading
        (102, 780, 92),  // Normal reading
        (101, 1250, 90), // Exceeds threshold -> alert
        (103, 520, 88),  // Normal reading
        (102, 2100, 85), // Critical threshold -> alert
    ];

    for (sensor_id, value, quality) in sensor_data {
        let reading = SensorReading {
            sensor_id,
            value,
            timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            quality,
        };

        sensor_pub.publish(reading).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    println!("\n=== Example Complete ===");
    println!("✨ Generic pub/sub - no domain-specific terminology");
    println!("   Works for: IoT sensors, industrial monitoring, smart homes, etc.");
}
