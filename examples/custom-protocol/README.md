# Custom Protocol Example

This example demonstrates how to use `mycelium-codegen` to generate your own custom protocol messages.

## What This Shows

- How to define custom messages in `contracts.yaml`
- How to use `mycelium-codegen` in your `build.rs`
- How to send/receive custom messages with zero-copy serialization
- How to use buffer pools for zero-allocation message handling

## Project Structure

```
custom-protocol/
├── Cargo.toml          # Dependencies including mycelium-codegen
├── build.rs            # Build script that calls codegen
├── contracts.yaml      # YOUR custom message definitions
└── src/
    └── main.rs        # Example using generated messages
```

## How It Works

### 1. Define Messages (contracts.yaml)

```yaml
messages:
  PlayerLogin:
    type_id: 1000
    description: "Player login request"
    fields:
      - name: player_id
        type: u64
      - name: session_token
        type: u64
```

### 2. Configure Build Script (build.rs)

```rust
fn main() {
    println!("cargo:rerun-if-changed=contracts.yaml");

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let output_path = format!("{}/generated_messages.rs", out_dir);

    mycelium_codegen::generate_from_yaml("contracts.yaml", &output_path)
        .expect("Failed to generate protocol messages");
}
```

### 3. Add Dependencies (Cargo.toml)

```toml
[dependencies]
mycelium-protocol = { path = "../../crates/mycelium-protocol" }
zerocopy = { version = "0.7", features = ["derive"] }

[build-dependencies]
mycelium-codegen = { path = "../../crates/mycelium-codegen" }
```

### 4. Use Generated Messages (src/main.rs)

```rust
// Include generated code
include!(concat!(env!("OUT_DIR"), "/generated_messages.rs"));

// Create and send messages
let login = PlayerLogin {
    player_id: 12345,
    session_token: 0xDEADBEEF,
    timestamp: 1234567890,
};

codec::write_message(&mut stream, &login).await?;
```

## Running the Example

```bash
cargo run --example custom-protocol
```

## What Gets Generated

For each message in `contracts.yaml`, the codegen creates:

1. **Struct Definition** - Zero-copy compatible with `zerocopy` derives
2. **Message Trait Implementation** - `TYPE_ID` and category
3. **Buffer Pool Configuration** - Optimal buffer sizes for your messages

Example generated code:

```rust
#[derive(Debug, Clone, Copy, PartialEq, IntoBytes, FromBytes, FromZeros)]
#[repr(C)]
pub struct PlayerLogin {
    pub player_id: u64,
    pub session_token: u64,
    pub timestamp: u64,
}

impl Message for PlayerLogin {
    const TYPE_ID: u16 = 1000;
    fn category(&self) -> &'static str {
        "custom"
    }
}
```

## Supported Field Types

- Integers: `u8`, `u16`, `u32`, `u64`, `u128`, `i8`, `i16`, `i32`, `i64`, `i128`
- Floats: `f32`, `f64`
- Boolean: `bool`
- Large integers: `U256` (from primitive-types)

## Benefits

- **Zero-Copy Serialization**: No allocation during encoding/decoding
- **Type Safety**: Compile-time guarantees for message structure
- **Fast Build Times**: Code generation runs only when contracts.yaml changes
- **Buffer Pooling**: Automatic pool configuration for optimal performance

## Integration

To use this in your own project:

1. Copy the pattern from this example
2. Define your own `contracts.yaml`
3. Add `mycelium-codegen` as a build dependency
4. Use generated messages with Mycelium transport layer

That's it! You get high-performance protocol messages without writing boilerplate.
