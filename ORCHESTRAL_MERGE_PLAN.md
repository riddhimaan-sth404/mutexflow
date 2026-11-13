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

### Phase 2 — Build and compatibility baseline
- Run Rust and Python tests from their imported locations.
- Resolve path assumptions, environment-variable names, and Windows/Linux process-launch differences.
- Add a reproducible runtime setup script; do not silently skip missing sandbox dependencies.

### Phase 3 — Contract and adapter
- Define a versioned internal API for starting, cancelling, and observing workflow runs.
- Add a MutexFlow backend adapter; never expose Orchestral admin credentials to the desktop.
- Use least-privilege agent tokens and target scopes. High-impact actions must remain approval-gated.

### Phase 4 — Desktop integration
- Add workflow status, agent activity, approval prompts, findings, and audit history to MutexFlow UI.
- Keep local/cloud inference configuration in MutexFlow and pass only necessary provider configuration to the runtime.

### Phase 5 — Security and persistence
- Decide ownership/migration for SQLite state versus Supabase product data.
- Keep audit records append-only and hash-chained; add verification tests and explicit key-management/rotation design.
- Harden loopback binding, CORS, authentication, process permissions, secret storage, and sandbox availability.

### Phase 6 — Validation and release
- Build/test Rust and Python components independently.
- Add end-to-end tests for workflow execution, scope rejection, approval-required actions, cancellation, event streaming, and audit verification.
- Test Windows first, then Linux; document unavailable sandbox features rather than weakening isolation.
- Deprecate standalone Orchestral packaging only after parity and migration tests.

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
