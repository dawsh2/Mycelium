# EXTENSIONS ROADMAP

This document tracks future-looking perf/ergonomics experiments. Each item is
optional and can be implemented behind feature flags so core Mycelium semantics
remain stable.

## Batching for Unix/TCP

- **Rationale**: Standard technique to amortize syscalls for high-volume
  deployments (e.g., processing a full block of on-chain events before
  broadcasting). Lets trading stacks publish one batch per block instead of
  per-event chatter.
- **Plan**:
  1. Layer a batching buffer on top of `StreamPublisher` that reuses
     `BufferPool` storage.
  2. Flush based on size and timer thresholds (configurable in
     `TransportConfig`).
  3. Teach the codec to read multiple TLV frames per buffer.
  4. Provide preset profiles (low-latency vs. high-throughput) with benchmarks.

## Specialized Bounded Queues

- **Rationale**: Actor mailboxes currently lean on `tokio::mpsc`. A cache-friendly
  ring buffer (SPSC/MPMC) can cut latency for risk-sensitive flows.
- **Plan**: Abstract queue choice, implement a lock-free ring, expose it as an
  opt-in mailbox builder, and stress-test cancellation/backpressure behavior.

## Dual-Path Routing Revisited

- **Current Capability**: Compile-time routing already gives a built-in fast
  path—critical services live in a monolith and call each other directly while
  the message bus handles slower fan-out. The existing primitives therefore
  already provide “dual path” behavior without new infrastructure.
- **Possible Extension**: If we need per-message fast handlers inside the same
  runtime, we can register synchronous callbacks before publishing to the
  broadcast channel. No action required until real workloads demand it.

## Shared-Memory Transport

- **Rationale**: Deliver Arc-level latency across processes on the same box.
- **Plan**: Define shared-memory endpoints in topology config, implement a
  lock-free ring buffer backed by `memfd`/`mmap`, and add crash-safe
  handshakes/cleanup.

## Pinned Executors

- **Rationale**: Provide deterministic scheduling for HFT adapters by pinning a
  service to a dedicated core.
- **Plan**: Add execution policies to `ServiceRuntime`, spin up single-thread
  Tokio runtimes when requested, and manage lifecycle/shutdown hooks.

