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

**Managed integration is wired into MutexFlow.** The Rust Citadel core and restricted Python local-analysis worker are managed sidecars launched by the MutexFlow Rust backend. The backend also hosts the JCode Harness SDK adapter for specialist agent sessions, bounded concurrent batches, live events, cancellation, workspace confinement, and Citadel audit mediation. The existing Avalonia application is the user-facing dashboard. The imported general-purpose VigilEngine tool runtime remains preserved as source but is not exposed through the desktop adapter; active scanning, shell, writes, network, and MCP tools are intentionally not enabled.

## Upstream

- Repository: https://github.com/riddhimaan-sth404/orchestral
- Branch: `main`
- Snapshot: `6b27d6bd3a05fb9a01a61259aa1a2d4113fce252`
