# JCode runtime integration

## Decision

Integrate JCode through its supported Rust SDK and versioned Harness API, not by copying its entire application or replacing Orchestral. JCode is a high-performance agent harness; it is not a drop-in replacement for the Citadel policy/control plane or VigilEngine's restricted analysis worker.

Upstream inspected: https://github.com/1jehuang/jcode  
License inspected: MIT  
Pinned compatibility snapshot: `72f9c07ff59639221b9f371b759f784416bd1d12` (2026-11-13)

## Responsibility split

- **MutexFlow / Avalonia:** remains the only desktop UX and configuration surface.
- **Orchestral / Citadel:** remains authoritative for workflow identity, workspace scope, policy decisions, approval gates, audit records, cancellation, and tool authorization.
- **JCode:** supplies the agent session/runtime engine and its multi-session scheduling/runtime optimizations.
- **Adapter:** connects to JCode through `jcode-sdk` / the versioned Harness API and translates events into Orchestral's versioned workflow contract.

Do not import JCode's TUI or provider UI into the desktop. Do not create a second user-facing control plane.

## Required security boundary

1. Launch JCode as an isolated, managed instance with a dedicated application data directory.
2. Set credential inheritance off. Never import the user's interactive JCode home or credentials implicitly.
3. Keep the JCode API transport local-only (Unix socket or Windows named pipe); do not expose it on a network interface.
4. Configure each session with an explicit tool allowlist. The safe initial configuration is an empty built-in tool set.
5. Custom tool calls return to the MutexFlow backend adapter. The currently exposed tools are read-only workspace listing and UTF-8 file reading, with canonical-root confinement, traversal rejection, symlink rejection, and strict output limits.
6. Citadel-mediated audit/approval for state-changing tools is not yet implemented. Keep writes, shell/process execution, network tools, MCP, self-development, and autonomous external actions disabled until that adapter exists.
7. Treat model output and JCode events as untrusted input. Bound message sizes, file reads, directory listings, execution time, and concurrent sessions.
8. Store provider credentials only through an explicit user-controlled credential flow. Never pass the MutexFlow admin key, JWT secret, Supabase credentials, or unrelated environment variables to JCode.

## Integration stages

### J0 — Upstream contract baseline (complete)
- Pin the upstream commit used by CI.
- Compile and test the SDK, stable Harness API, and API bridge on Linux and Windows.
- Keep this compatibility check separate from MutexFlow's own test suite.

### J1 — Managed runtime adapter (implementation added; CI validation pending)
- Pin and depend on the upstream `jcode-sdk` crate at the inspected commit.
- Launch JCode's API bridge as a backend-owned child process with a clean environment, isolated `JCODE_HOME`, and no inherited provider login files.
- Connect through the supported Rust SDK with automatic shared-runtime startup disabled.
- Add backend endpoints for runtime health, session listing/creation, concurrent message turns, and cancellation.
- Configure each new session with an explicitly empty built-in tool allowlist and two backend-executed custom read-only tools (`workspace_read` and `workspace_list`).
- Bound concurrent turns to eight and prompt payloads to 64 KiB.
- Terminate the managed JCode process when the backend runtime is dropped.
- The SDK's built-in `launch_instance` helper is intentionally not used because its launch options inherit the parent process environment; MutexFlow instead launches the bridge with an explicit environment allowlist.

### J2 — Orchestral policy mediation (partial)
- Built-in JCode tools remain disabled.
- Backend custom tools provide confined, read-only workspace listing and file reading; tests cover traversal rejection and basic reads.
- State-changing tools, shell, network, MCP, and native autonomous swarm workers remain disabled until Citadel policy evaluation, approval, audit, and sandboxing are integrated.

### J3 — Avalonia presentation (pending)
- Add JCode runtime status and session controls to the existing Orchestral dashboard.
- Render streamed output and cancellation state; never expose raw IPC endpoints or secrets.
- Surface actionable errors when the JCode binary, provider setup, or runtime is unavailable.

### J4 — Performance and release gate
- Compare Orchestral-only and Orchestral+JCode on identical hardware, models, prompts, workspaces, and concurrency.
- Measure cold/warm startup, time-to-first-event/token, p50/p95 turn latency, throughput, peak RSS, incremental RSS per session, cancellation latency, and crash recovery.
- Run at least 30 samples per benchmark case; publish raw samples and environment metadata.
- Require functional parity and no security-boundary regressions before enabling JCode by default.
- Report speedups only from measured results. A 12x improvement is a target to investigate, not an assumed outcome.

## Current status

J0 is implemented. J1 backend integration and J3 Avalonia controls are implemented in the branch and awaiting CI validation. J2 is partial: only confined read-only tools are exposed; Citadel-mediated state-changing tools and native JCode swarm execution remain incomplete. J4 measured performance/release gates remain incomplete. The JCode runtime is disabled by default, requires an explicitly configured binary, and is only enabled when the backend binds to loopback. No speedup claim is made.
