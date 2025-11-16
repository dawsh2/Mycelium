# Agentics on Mycelium

Mycelium already provides the essentials for an agentic control plane: typed channels, a unified message bus, lifecycle
management, and multiple transports. To evolve it into an agent-oriented framework, extend the base layer with the following
capabilities:

- **Plan / Execution API:** define TLVs for agent tasks (e.g., arbitrage scans, backtests, risk checks) so agents can issue
  asynchronous requests and receive structured responses.
- **Scheduler Primitives:** add a service that can spawn sub-tasks, coordinate multiple services, and aggregate their
  results—essential for planning loops and workflow orchestration.
- **Persistent Knowledge Store:** tap existing Redis/Postgres layers (or add similar stores) to track long-lived context,
  letting agents reason across restarts and adapt their strategies over time.

These additions let you keep Mycelium’s high-performance message passing while layering agent-style planning and decision
making on top.
