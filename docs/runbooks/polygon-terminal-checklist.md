# Polygon Terminal Production Readiness Checklist
Timestamp: 2025-11-10 14:50 PT

## 1. Bring Up Full Bandit Stack
1. From `/Users/daws/repos/Bandit`, build and run `polygon_adapter` (it launches listener, pool-state, flash-arb, execution-filter, executor-stub, terminal UI, etc.):
   ```bash
   cargo run --bin polygon_adapter --release
   ```
2. Confirm environment vars (RPC URLs, Redis, bus sockets) in `.env` or shell match staging/production-like settings.

## 2. Replay Captures Through Mycelium
1. In the same repo, use the replay harness with a capture file (adjust path):
   ```bash
   cargo run -p polygon-replay-harness --release -- \
     --input captures/polygon_swaps_2025-10-01.tlv \
     --speed 1.0
   ```
2. Ensure the harness points to the same MessageBus endpoint as the running stack.
3. Let the replay run for at least 30–60 minutes to simulate steady traffic.

## 3. Monitor Backpressure / Health
While the replay runs, open another terminal and:
1. Tail terminal UI logs for dropped events/backpressure warnings:
   ```bash
   journalctl -fu polygon-terminal | rg 'drop|backpressure|lag'
   ```
2. Watch overall service logs for queue growth:
   ```bash
   journalctl -fu polygon-listener polygon-pool-state polygon-flash-arbitrage \
     | rg 'warn|error'
   ```
3. If available, run `tokio-console` (or similar) against polygon-terminal to observe task latency/queue depths.

## 4. Log Review & Instrumentation Checks
1. Set log level to `INFO` (or `DEBUG` for hydration) via env var (e.g., `RUST_LOG=info,polygon_terminal=debug`).
2. During the run, verify logs report:
   - Redis hydration success + duration.
   - Event subscription status (connected/disconnected).
   - Metrics snapshots (Strategy / Executor) being received.
3. After replay completes, grep for WARN/ERROR:
   ```bash
   journalctl -u polygon-terminal --since '-2h' | rg 'WARN|ERROR'
   ```

## 5. Fault Injection Scenarios
Perform each while replay is active; observe logs and recovery:

1. **Pool-State Manager outage**
   ```bash
   systemctl stop polygon-pool-state
   sleep 60
   systemctl start polygon-pool-state
   ```
   Check terminal logs for clear error + recovery messages.

2. **Redis outage after warm start**
   ```bash
   systemctl stop redis
   sleep 60
   systemctl start redis
   ```
   Ensure terminal logs only warn when new pools need metadata.

3. **Bus disruption**
   Temporarily block the bus port (e.g., using `iptables` or shutting down the bus process) and confirm reconnect logs appear.

## 6. Operator Runbook & Observability Validation
1. Record the exact commands/log filters used above into this runbook as you execute them.
2. Confirm key telemetry (events/sec, dropped events, UI render latency, hydration duration) is exported to Prometheus/Grafana or logged with structured fields.
3. Verify `polygon-terminal` exposes or logs:
   - `hydration_duration_ms`
   - `event_queue_depth`
   - `event_drop_count`
   - `ui_frame_ms`
4. If any metrics are missing, note them for follow-up instrumentation.

## 7. Completion Criteria
- No unexplained WARN/ERROR entries during replay + fault scenarios.
- Terminal UI recovers cleanly from each injected fault.
- Replay throughput matches expected production load without backpressure alerts.
- Runbook updated with findings and log commands.
