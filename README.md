# Mycelium <img src="docs/assets/cultures.svg" alt="Mycelium" width="48" style="vertical-align: middle; margin-bottom: -8px;">

Typed message bus for Rust services that can run in a single process or across
multiple processes without changing application code. Mycelium keeps
message-shape definitions, transport selection, and actor supervision in one
place so the same service logic works with shared memory (Arc), Unix sockets, or
TCP.

- Messages are defined once in `contracts.yaml`. Code generation produces Rust
  structs with serialization, validation, and TLV encoders.
- Services are plain structs annotated with `#[service]`. The macro wires in a
  `ServiceContext` that exposes async publishers/subscribers plus tracing and
  metrics hooks.
- `MessageBus` encapsulates the transport. Swap the transport at startup; the
  service code stays untouched.
- For single-process deployments you can bypass the bus entirely with
  `routing_config!`, which generates compile-time routers that issue direct
  function calls (no Arc, no heap, ~2–3 ns per handler).

## Core pieces

| Component | Location | Role |
| --- | --- | --- |
| `contracts.yaml` | project root | Source of truth for message schemas. The build script in `crates/mycelium-protocol` generates strongly typed Rust APIs. |
| `ServiceContext` | `crates/mycelium-transport/src/service.rs` | Runtime handle passed to every service method. Provides `emit`, `subscribe`, shutdown signals, metrics, and tracing spans. |
| `MessageBus` | `crates/mycelium-transport/src/bus.rs` | Transport abstraction. Supports `Arc` (single process), Unix domain sockets, and TCP. |
| `ServiceRuntime` | `crates/mycelium-transport/src/runtime.rs` | Owns task supervision. Spawns `#[service]` actors, applies exponential backoff, and coordinates shutdown. |
| `routing_config!` | `crates/mycelium-transport/src/routing.rs` | Optional compile-time router that maps message types to handlers with direct function calls. |

## Deployment options

| Use case | Transport | How to configure |
| --- | --- | --- |
| All services in one binary (lowest latency) | `MessageBus::new()` (Arc) | Pass the same `Arc<MessageBus>` to every service and spawn them in one `ServiceRuntime`. |
| Multiple binaries on one host | `MessageBus::with_unix_transport(path)` | Start a listener process with the Unix transport; other processes connect to the same path. |
| Cross-host / distributed | `MessageBus::with_tcp_transport(addr)` | Run a TCP listener and point clients at the host:port. |

Switching between the three changes only the `MessageBus` constructor; the
service code and generated message types are identical.

## Quick start

1. **Describe messages.** Edit `contracts.yaml`:

   ```yaml
   messages:
     GasMetrics:
       tlv_type: 200
       domain: telemetry
       fields:
         block_number: u64
         base_fee_per_gas: "[u8; 32]"
   ```

2. **Build once.** `cargo build` regenerates `bandit_messages::GasMetrics` (and
   the associated `Message` trait impl) during the build script phase.

3. **Write a service.**

   ```rust
   use anyhow::Result;
   use mycelium_transport::{service, ServiceContext};

   pub struct GasLogger;

   #[service]
   impl GasLogger {
       async fn run(&mut self, ctx: &ServiceContext) -> Result<()> {
           let mut sub = ctx.subscribe::<bandit_messages::GasMetrics>().await?;

           while let Some(metric) = sub.recv().await {
               tracing::info!(block = metric.block_number, "gas metric");
           }

           Ok(())
       }
   }
   ```

4. **Choose a transport and spawn.**

   ```rust
   use mycelium_transport::{MessageBus, ServiceRuntime};

   #[tokio::main]
   async fn main() -> Result<(), anyhow::Error> {
       let bus = MessageBus::new();            // swap for Unix/TCP as needed
       let runtime = ServiceRuntime::new(bus);

       runtime.spawn_service(GasLogger).await?;

       tokio::signal::ctrl_c().await?;
       runtime.shutdown().await?;
       Ok(())
   }
   ```

## Observability and resilience

- `ServiceContext::emit` attaches tracing spans and counters. The overhead is ~120
  ns with instrumentation enabled, ~65 ns without.
- Services can call `ctx.is_shutting_down()` or register shutdown hooks to stop
  gracefully when the runtime receives a termination signal.
- Failures inside a service task trigger exponential backoff restarts. The
  backoff parameters live in `crates/mycelium-transport/src/runtime.rs`.

## When to use compile-time routing

`routing_config!` generates a struct that routes messages by direct function
calls (no dynamic dispatch, no `Arc<T>`). It gives you monolith-level latency
with ~2–3 ns per handler invocation. Use it when you run fully single-process
and need the absolute floor on overhead. For anything that crosses process
boundaries or prefers dynamic subscriptions, stick with the runtime bus.

## Testing tips

- Services implement pure structs; in unit tests you can instantiate them and
  drive the async methods with `tokio::test` plus a mock `MessageBus` (use the
  Arc transport and issue messages directly).
- Integration tests often start a `ServiceRuntime` with the Arc transport to keep
  things fast, even if production swaps to Unix/TCP.

## FAQ

**Does each process need the same `contracts.yaml`?** Yes. Generated code is part
of the workspace, so keep schemas in sync across binaries.

**Can transports mix?** You can run multiple transports simultaneously (e.g., Arc
inside a binary with a TCP bridge), but most deployments pick one per runtime.

**Where are benchmarks?** See `docs/perf/README.md` for the latest latency
measurements across transports and routing modes.
