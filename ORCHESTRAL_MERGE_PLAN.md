# MutexFlow + Orchestral Integration Plan

## Goal

Make MutexFlow the user-facing desktop AI workflow product and integrate Orchestral as its independently testable agent execution and security runtime.

This is an incremental integration, not a destructive repository replacement. Existing MutexFlow desktop, AI provider cascade, licensing, OAuth, and integration functionality must remain available throughout migration.

## Ownership

- **MutexFlow:** Avalonia/.NET 9 desktop UX, workflow authoring/presentation, local model management, inference-provider configuration, licensing, OAuth, and user-facing integration setup.
- **Orchestral:** agent lifecycle/scheduling, tool registry/execution, scoped authorization, target-boundary checks, human approval gates, coordinated workers, shared blackboard, and audit/event infrastructure.
- **Shared contract:** versioned types for workflow runs, agent tasks, tool invocations, policy decisions, approval requests, events, and audit references.

## Target architecture

```text
MutexFlow Desktop (Avalonia / .NET 9)
             |
             v
MutexFlow Rust API (existing app API)
  - licensing / OAuth / provider cascade
  - workflow and integration endpoints
  - Orchestral adapter
             |
             v
Orchestral Runtime
  - Citadel control plane (Rust / Axum)
  - VigilEngine worker (Python, initially)
  - tool registry and scoped execution
  - approval gates and audit ledger
             |
             v
Local models / cloud providers / approved connected tools
```

The initial integration keeps Citadel as a loopback-only sidecar to preserve behavior and enable independent testing. The long-term decision between an embedded Rust library and managed sidecar comes only after API and lifecycle tests exist. Avoid maintaining two independent public control planes indefinitely.

## Phases

### Phase 0 — Reconnaissance (complete)
- Inspected both default branches, repository layouts, runtime languages, API surfaces, and desktop projects.
- Identified duplicate HTTP control planes and preserved product-specific routes.

### Phase 1 — Safe code import (complete)
- Created `integration/orchestral-runtime` in MutexFlow.
- Imported the Orchestral Citadel Rust core, Python worker, recipes, CLI, and tests under `orchestral-runtime/`.
- Kept imported code in its own package boundaries and did not overwrite MutexFlow source.
- Added a read-only MutexFlow backend health proxy at `GET /api/v1/orchestral/health`, configurable with `ORCHESTRAL_CORE_URL`.

### Phase 2 — Managed runtime lifecycle (implemented; CI validation tracked by PR)
- Added a Tokio process manager for Citadel and a dedicated local-analysis worker.
- The backend starts the processes only when `ORCHESTRAL_ADMIN_KEY` is configured and autostart is enabled.
- Both services bind to loopback; the admin key is kept in the backend/child-process environment and is not returned to the desktop.
- Added graceful child-process shutdown when the backend receives Ctrl+C.
- Added startup readiness reporting; the Windows launcher waits for backend readiness before opening the desktop.
- Backend binds to loopback by default, restricts browser CORS to loopback origins, and validates sidecar URLs to prevent admin-key forwarding to remote hosts.
- Citadel data is stored under the ignored `.mutexflow/orchestral/` directory.

### Phase 3 — Versioned backend API and safe worker adapter (implemented)
- Added backend routes for runtime status, agents, findings, pending approvals, approval/denial, audit events, and local-analysis jobs.
- Added a worker service that exposes only passive local workspace secret scanning, with path canonicalization and workspace-root confinement.
- Findings are submitted to Citadel with masked evidence only.
- Added cooperative cancellation for local workspace scans.
- The full VigilEngine autonomous worker is preserved but is not exposed through this adapter.

### Phase 4 — Desktop integration (implemented; CI validation tracked by PR)
- Added an Orchestral desktop section for runtime status, local analysis, agents, findings, pending approvals, and audit events.
- Added explicit approve/deny controls for pending requests.
- Existing MutexFlow AI inference modes and integration flows remain separate and unchanged.

### Phase 5 — Security and persistence (partial)
- Runtime services bind to loopback; worker requests require a shared secret header.
- Workspace scans reject absolute paths and traversal outside the configured root; the existing scanner also excludes sensitive system and credential paths.
- Citadel uses a local SQLite database under the ignored application data directory.
- Configure a stable `ORCHESTRAL_JWT_SECRET`; without it Citadel intentionally creates an ephemeral signing secret at startup.
- Remaining hardening includes OS-native secret storage for the backend's Orchestral admin key, stronger process sandboxing, and explicit audit-chain verification/key-rotation work.

### Phase 6 — Validation and release (CI-backed; release hardening remains)
- CI covers both Rust projects, Python worker syntax/guard tests, and Avalonia desktop build on Linux and Windows.
- Added unit tests for loopback URL validation, job ID validation, workspace traversal/symlink rejection, masked secret evidence, and cooperative cancellation.
- Added a managed-runtime end-to-end test that starts backend-managed Citadel and worker processes, runs a local analysis job, reads agent/finding state, exercises both approval and denial through the backend, and retrieves audit events.
- The managed-runtime end-to-end test also sends Ctrl+C to the backend and verifies both sidecar health ports close.
- Remaining release hardening includes OS-native storage for the Orchestral admin key, stronger process sandboxing, and explicit audit-chain verification/key-rotation work. JCode restarts its managed child on the next request after a crash; Citadel/worker crash recovery is still a release-hardening item.
- General-purpose arbitrary tool dispatch remains out of scope. The JCode integration in Phase 8 supplies scoped independent agent sessions, live output events, role-selected concurrent batches, and cancellation; it does not enable shell, write, network, MCP, or active scanning tools.

### Phase 7 — Compatibility and rollout
- Keep standalone Orchestral packaging available while the new MutexFlow adapter matures.
- Deprecate standalone packaging only after feature parity, migration, and security review.

### Original implementation roadmap (historical notes)

- Run Rust and Python tests from their imported locations.
- Resolve path assumptions, environment-variable names, and Windows/Linux process-launch differences.
- Add a reproducible runtime setup script; do not silently skip missing sandbox dependencies.


## Non-goals
- Do not replace MutexFlow desktop with Orchestral Desktop.
- Do not delete or rewrite MutexFlow AI cascade, OAuth, licensing, or integrations.
- Do not merge databases or credential stores without a migration plan.
- Do not claim signatures/hash chains prove an agent's account is truthful; they provide integrity evidence for recorded data.
- Do not weaken scope enforcement or human approval gates for convenience.

## Upstream snapshot
- Repository: https://github.com/riddhimaan-sth404/orchestral
- Branch: `main`
- Snapshot commit/tree: `6b27d6bd3a05fb9a01a61259aa1a2d4113fce252`
- Imported into: `orchestral-runtime/`
- MutexFlow integration branch: `integration/orchestral-runtime`

Update completion status only after corresponding code and tests are verified.


## Phase 8 — JCode runtime integration (implementation complete; performance target unverified)

- Pinned the upstream MIT-licensed JCode SDK/runtime snapshot at `72f9c07ff59639221b9f371b759f784416bd1d12`.
- Added a managed Rust runtime with a dedicated JCode home, clean child environment, loopback-only startup, provider selection, SDK-based owner-only credential provisioning, health checks, and restart-on-next-use after runtime failure.
- Added Citadel-backed agent registration/status updates and chained audit records for agent lifecycle and every custom tool request/result. Tool execution fails closed when request audit is unavailable; results are withheld if result audit cannot be committed.
- Added four fixed specialist roles and up to eight concurrent independent JCode sessions.
- Added bounded live event delivery, session listing, cancellation, usage/latency reporting, and Avalonia controls for role selection, objectives, streaming output, and batch launches.
- Exposed only confined read-only workspace read/list/search tools. Built-in shell, write, network, MCP, active scanning, and terminal tools remain disabled.
- Added an audit-event write endpoint to Citadel and updated the integration/validation documentation.
- Added JCode SDK/API compatibility CI on Linux and Windows, plus pinned-binary builds and isolated runtime lifecycle smoke tests on both operating systems.
- No 12x speedup is claimed until a reproducible same-hardware/provider benchmark produces raw measurements. JCode's native terminal UI and the original Orchestral active security tool suite are not represented as integrated; the Avalonia UI is the product interface and the enabled agent toolset is deliberately read-only.
