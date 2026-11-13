# Orchestral Runtime

This directory contains an imported snapshot of the Orchestral runtime, integrated
into MutexFlow without replacing MutexFlow's existing backend or desktop client.

## Layout

- `core/`: Rust/Axum Citadel control plane with scoped API-key and agent-token
  authentication, target-scope enforcement, SQLite persistence, HITL endpoints,
  blackboard APIs, audit-log access, and SSE/WebSocket telemetry.
- `worker/`: Python VigilEngine worker, guardrails, supervisor, tools, MCP server,
  coordinated swarm, and terminal observatory.
- `recipes/`: canonical agent workflow recipes.
- `tests/`: Orchestral unit and integration tests.
- `bin/orchestral`: standalone Orchestral CLI entry point.

The Orchestral desktop project is intentionally not imported: MutexFlow's existing
Avalonia desktop remains the product UI.

## Integration status

**Source imported; runtime not yet wired into MutexFlow.** The Rust core and Python
worker retain their separate package boundaries. The next stage is to establish
build/test baselines, then introduce a versioned adapter in the existing MutexFlow
backend. Do not assume that the imported runtime is already launched by
`run.bat` or invoked by the desktop app.

## Upstream

- Repository: https://github.com/riddhimaan-sth404/orchestral
- Branch: `main`
- Snapshot: `6b27d6bd3a05fb9a01a61259aa1a2d4113fce252`
