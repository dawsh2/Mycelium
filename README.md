# Mycelium <img src="docs/assets/cultures.svg" alt="Mycelium" width="48" style="vertical-align: middle; margin-bottom: -8px;">

Typed, schema-first plumbing for trading systems and other low-latency services.
Define messages once, generate codecs for Rust/Python/OCaml, and choose how data
moves—shared memory, Unix sockets, TCP, or native FFI—without rewriting
business logic.

## Why Mycelium?

- **Single source of truth** – `contracts.yaml` feeds codegen for every runtime.
- **Configurable transports** – swap Arc/Unix/TCP bridges per node; services stay
  unchanged.
- **Polyglot in-process access** – Python (PyO3) and OCaml embed the Rust bus via
  `mycelium-ffi`, so non-Rust code can publish/subscribe without a bridge hop.
- **Production hardening** – `ServiceRuntime` supervises tasks, applies restart
  backoff, emits telemetry, and exposes health checks.

## Architecture at a glance

```
                 ┌──────────────┐  Unix/TCP bridge  ┌──────────────┐
                 │   Node A     │◀───────────────▶│    Node B    │
                 │ (Rust bus)   │                  │ (Rust bus)   │
                 └──────┬───────┘                  └──────┬───────┘
                        │                                   │
         in-process FFI │                                   │ in-process FFI
                        │                                   │
                ┌───────▼───────┐                     ┌─────▼──────┐
                │Python Native  │                     │OCaml Native│
                │PyO3 adapter   │                     │Runtime     │
                └───────────────┘                     └────────────┘

Messages are described once; each node decides whether to run entirely in
process (Arc transport), expose a Unix/TCP bridge, or mix the two. Native
adapters talk to the same `MessageBus`, so Python/OCaml code can live in the
critical path when needed.
```

## Core components

| Piece | Location | Role |
| --- | --- | --- |
| Schema (`contracts.yaml`) | repo root | Defines TLV IDs, topics, and field types. Build scripts generate Rust types; codegen emits Python/OCaml bindings for bridges/native mode. |
| `ServiceContext` | `crates/mycelium-transport/src/service.rs` | Handle passed to each `#[service]` impl. Provides publishers/subscribers, metrics, tracing, and shutdown signals. |
| `MessageBus` | `crates/mycelium-transport/src/bus.rs` | Transport abstraction (Arc / Unix / TCP). Handles publishers, fan-out, bridge taps. |
| `ServiceRuntime` | `crates/mycelium-transport/src/service_runtime.rs` | Supervises async services, restarts failures with exponential backoff, and coordinates shutdown. |
| `routing_config!` | `crates/mycelium-transport/src/routing.rs` | Optional compile-time router for single-process setups (~ns latency). |
| Native adapters | `crates/mycelium-python-native`, `ocaml-sdk/` | PyO3 extension + OCaml runtime that call `mycelium-ffi` for in-process interop. |

## Quick start (Rust)

1. **Describe messages.**

   ```yaml
   messages:
     GasMetrics:
       tlv_type: 200
       domain: telemetry
       fields:
         block_number: u64
         base_fee_per_gas: "[u8; 32]"
   ```

2. **Generate/Build.** `cargo build` runs the protocol build script and exports
   `bandit_messages::GasMetrics` (or whatever your schema names).

3. **Implement a service.**

   ```rust
   use mycelium_transport::{service, ServiceContext};

   pub struct GasLogger;

   #[service]
   impl GasLogger {
       async fn run(&mut self, ctx: &ServiceContext) -> anyhow::Result<()> {
           let mut sub = ctx.subscribe::<bandit_messages::GasMetrics>().await?;
           while let Some(metric) = sub.recv().await {
               tracing::info!(block = metric.block_number, "gas metric");
           }
           Ok(())
       }
   }
   ```

4. **Choose transport at startup.**

   ```rust
   #[tokio::main]
   async fn main() -> anyhow::Result<()> {
       let bus = mycelium_transport::MessageBus::new(); // Arc transport
       let runtime = mycelium_transport::ServiceRuntime::new(bus);
       runtime.spawn_service(GasLogger).await?;
       tokio::signal::ctrl_c().await?;
       runtime.shutdown().await?;
       Ok(())
   }
   ```

Swap `MessageBus::new()` for the Unix/TCP helpers when you want out-of-process
routes; service code stays the same.

## Native runtimes (Python & OCaml)

Source `scripts/env/native.sh` to configure the shared CPython + PyO3
`RUSTFLAGS` + opam switch:

```bash
source scripts/env/native.sh
cargo test -p mycelium-python-native
cd ocaml-sdk && dune build
```

```
┌──────────┐      TLV bytes + callbacks       ┌───────────────────────┐
│ Python   │ publish/subscribe via PyO3/FFI ─▶│  mycelium-ffi (C ABI) │
│ or OCaml │─────────────────────────────────▶│  mycelium_runtime_*   │
└──────────┘                                 └──────────┬────────────┘
                                                       │ Rust boundary
                                                       ▼
                                             ┌────────────────────────┐
                                             │ MessageBus / Tokio /   │
                                             │ existing Rust services │
                                             └────────────────────────┘
```

### Python
- PyO3 extension crate lives in `crates/mycelium-python-native`.
- Tests run via `cargo test -p mycelium-python-native` once the env helper is
  sourced.
- `pipx run --spec maturin maturin build --release` (or the CI job) outputs
  `target/wheels/mycelium_native-*.whl`.

### OCaml
- Native runtime + C stubs live in `ocaml-sdk/lib`.
- `cargo build -p mycelium-ffi --release` must run first to produce
  `target/release/libmycelium_ffi.dylib`.
- `cd ocaml-sdk && opam exec -- dune build` builds the OCaml library and tests.

## Deployment patterns

| Scenario | Recommendation |
| --- | --- |
| Single monolith | `MessageBus::new()` (Arc). Optionally use `routing_config!` for direct call routing. |
| Same host, different processes | Start a Unix bridge via `MessageBus::bind_unix_endpoint_with_digest`, have workers use `Mycelium.Transport.connect_unix`. |
| Cross-host cluster | Use TCP bridges (`bind_tcp_endpoint`). Each worker chooses the nearest transport; Python/OCaml can connect via sockets or native FFI. |
| Mixed workloads | Combine Arc (in-process actors) with Unix/TCP fan-out. Native adapters can embed the bus when they share the process, or fall back to sockets for isolation. |

## Operational notes

- `ServiceRuntime` restarts tasks with exponential backoff; hook into
  `ctx.is_shutting_down()` for graceful exits.
- All publishers/subscribers understand TLV envelopes (`Envelope` carries
  sequence, trace IDs, destination hints).
- Telemetry hooks live in `ServiceContext`, `ServiceMetrics`, and the bridge
  stats collectors.

## Development workflow

1. `cargo fmt && cargo clippy --workspace --all-features -- -D warnings`
2. `cargo test --workspace` (includes doc tests).
3. For native mode: `source scripts/env/native.sh`, rebuild `mycelium-ffi`, run
   `dune build`, then `cargo test -p mycelium-python-native`.
4. Use `scripts/run_native_checks.sh` (if sourced) to automate the above.

## FAQ

**Why not just ZeroMQ?** ZMQ gives you sockets; Mycelium gives you schema
discipline, typed codecs, transport supervision, and native adapters that reuse
the same runtime. You don’t reimplement schemas, tracing, or restarts per
language.

**Can I mix transports?** Yes. Nodes can host Arc actors, expose Unix/TCP bridges
for other processes, and embed Python/OCaml workers, all driven by the same
schemas.

**Benchmarks?** See `docs/perf/README.md` for Arc vs. Unix vs. TCP latency and
throughput numbers.
