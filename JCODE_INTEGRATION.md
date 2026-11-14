# JCode runtime integration

## Decision

JCode is integrated as the Rust agent-session runtime behind MutexFlow and Orchestral. It does not replace Citadel's policy/control plane, the existing passive local-analysis worker, or Avalonia. The integration uses JCode's supported Rust SDK and versioned Harness API rather than copying its TUI or creating a second desktop product.

- Upstream: https://github.com/1jehuang/jcode
- License: MIT (upstream LICENSE inspected)
- Pinned SDK/runtime snapshot: `72f9c07ff59639221b9f371b759f784416bd1d12`

## Responsibility split

- **MutexFlow / Avalonia:** desktop UX for provider credential setup/removal, agent roles, workspaces, session selection, live output, batch launches, and cancellation.
- **MutexFlow Rust backend:** owns the JCode child process, credential provisioning, concurrency gate, session adapter, event buffer, and workspace tool execution.
- **Orchestral / Citadel:** authoritative agent registry and append-only audit chain. Agent status transitions and every custom tool request/result are recorded through the authenticated Citadel admin API.
- **JCode:** Rust agent harness, model-provider runtime, session persistence, turn execution, and concurrent independent agent sessions.
- **VigilEngine Python worker:** remains available for its existing passive local-analysis workflow. JCode does not silently replace or bypass it.

## End-to-end workflow

1. The backend binds its local HTTP API first, then starts Citadel, the worker, and JCode in managed background tasks. Runtime health endpoints remain responsive while these services are starting. JCode's API bridge runs as an isolated child process with a dedicated `.mutexflow/jcode` home and local IPC socket.
2. Only the explicitly configured `JCODE_PROVIDER` and `JCODE_MODEL` settings are passed to the child. Provider API keys are not inherited in its environment.
3. Provider keys can be entered in the Avalonia dashboard and are sent only to the loopback backend. The backend provisions them through `JcodeClient::set_api_key`; JCode persists them in its owner-only credential store. MutexFlow stores only provider IDs in `.mutexflow/jcode/providers.json`, never key material, so provider readiness survives backend restarts. Supported key providers are OpenAI, Anthropic, OpenRouter, and Gemini.
4. Avalonia creates a role-scoped agent session. The backend creates the JCode session with a fixed Orchestral system prompt, configures its custom tool allowlist, and registers the agent in Citadel.
5. Up to eight independent JCode sessions can run concurrently. The batch API creates/registers each agent with the currently selected specialist role and schedules the turns through a bounded semaphore.
6. JCode text/tool events are translated into a bounded, sequence-numbered event buffer. Avalonia polls the event endpoint and renders assistant text as it arrives.
7. Each custom tool request and result is recorded in Citadel's audit chain. If the request audit fails, execution is denied; if the result audit fails, the result is withheld from the model. A run may also explicitly disable tools for a turn, which is used by the benchmark.
8. Agent completion, failure, and cancellation are reflected in Citadel. The desktop can cancel the selected session.

## Agent roles

The Avalonia UI offers four fixed roles:

- `workspace_analyst`
- `security_reviewer`
- `dependency_reviewer`
- `documentation_reviewer`

The role prompt is backend-authored. The user supplies an objective, but cannot replace the safety instructions or widen the workspace boundary.

## Tool and trust boundary

JCode built-in tools are disabled. The only custom tools exposed to sessions are:

- `workspace_read`: UTF-8 regular files up to 64 KiB.
- `workspace_list`: immediate directory entries, capped at 200.
- `workspace_search`: literal text search, capped at 1,000 files, 200 matches, and 1 MiB per file.

All tools are read-only and run in the MutexFlow backend, not inside the JCode process. They enforce relative paths, canonical workspace-root containment, symlink rejection, and exclusions for common credential files/directories, VCS metadata, and MutexFlow runtime state. Common API-key/password/secret assignments, bearer tokens, provider-token formats, and PEM private-key blocks are redacted from file contents and search previews before they reach the model. Audit records retain tool names, paths, and query lengths rather than storing search query text. The search tool skips generated/dependency directories such as `.git`, `target`, `node_modules`, `.venv`, and `vendor`.

Shell/process execution, file writes, network access, MCP, self-development, active port scanning, HTTP probing, and Kali terminal tools are not exposed. These modules remain vendored but hard-disabled: the managed worker's registry exposes only the passive secret scanner, and the standalone swarm runner exits unconditionally. No environment variable enables the dormant modules. Any future reintroduction requires a separately reviewed sandbox, explicit target authorization, Citadel policy evaluation, approval, and audit design. See [SECURITY.md](SECURITY.md).

## Lifecycle and failure behavior

- JCode autostart is disabled by default.
- Enable it with `JCODE_AUTOSTART=true` and set `JCODE_BINARY` to the installed executable.
- JCode autostart is blocked unless the backend binds to loopback (`127.0.0.1`, `localhost`, or `::1`).
- The API bridge runs with a clean environment and a dedicated state home. MutexFlow cloud, OAuth, Supabase, and Orchestral secrets are not inherited.
- Provider keys are passed only through the JCode SDK credential API and stored by JCode in its owner-only credential store. MutexFlow persists provider IDs only, never the key values.
- The manager checks the child and API health before reuse and attempts to restart the managed runtime on the next operation after a crash. Backend startup does not wait for sidecar compilation or JCode bridge startup; the UI can poll health while those tasks complete.
- Sessions are reconfigured with the restricted tool set before each turn because JCode tool configuration is memory-only.
- The event buffer retains up to 8,192 events globally and 1,024 per session. It is a live UI feed, not a durable transcript or audit ledger; Citadel remains the audit source.
- A turn reports provider usage counters and elapsed time when supplied by JCode. Cost is not fabricated when pricing metadata is unavailable.

## Configuration

Copy `backend-rust/.env.example` to `backend-rust/.env` and set:

- `JCODE_AUTOSTART=true`
- `JCODE_BINARY=/path/to/jcode` (or the Windows executable path)
- On Windows, build the matching pinned JCode Harness API bridge and set `JCODE_API_BRIDGE_BINARY` to its path, or place `jcode-harness-api-bridge.exe` beside `jcode.exe`. The upstream CLI api-bridge subcommand is Unix-only; MutexFlow starts the Windows daemon with `jcode server start` and hosts the same versioned API through this companion bridge.
- `ORCHESTRAL_ADMIN_KEY` and `ORCHESTRAL_JWT_SECRET` for the managed Citadel runtime
- One provider key: `JCODE_OPENAI_API_KEY`, `JCODE_ANTHROPIC_API_KEY`, `JCODE_OPENROUTER_API_KEY`, or `JCODE_GEMINI_API_KEY`
- `MUTEXFLOW_API_TOKEN` if you want to choose the local API token yourself; otherwise the backend generates one and stores it owner-only at `.mutexflow/backend-token`. Clients must send it as `X-MutexFlow-Token` on `/api/v1/orchestral/*` and `/api/v1/jcode/*` routes.

Optional `JCODE_PROVIDER` and `JCODE_MODEL` select the preferred provider/model. API-key provider identifiers are `openai-api`, `anthropic-api` (also accepted internally as `claude-api`), `openrouter`, and `gemini`; the backend infers the matching identifier when omitted. If no explicit provider is set, the backend infers one from configured keys in the order OpenAI, Anthropic, OpenRouter, Gemini. Existing `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `OPENROUTER_API_KEY`, and `GEMINI_API_KEY` variables are also accepted as provider-key sources.

Install the bundled Python worker dependencies once with `python -m pip install -e orchestral-runtime/worker`. The backend starts the worker process but does not install Python packages at runtime.

Never commit the real `.env` or credential files.

## Validation

The branch includes:

- JCode SDK/API compatibility checks on Linux and Windows.
- Linux and Windows builds of the pinned JCode executable, each followed by an isolated backend lifecycle smoke test. The Windows job also builds and exercises the pinned standalone Harness API bridge because the upstream CLI api-bridge subcommand is Unix-only.
- Rust tests for workspace path confinement, secret-file exclusion, and bounded event retention.
- Full MutexFlow/Orchestral CI, including backend tests, Citadel tests, Python worker tests, Avalonia build, Windows integration smoke, and managed-runtime end-to-end tests.

The Linux SDK job skips two upstream auth callback tests whose local-process fixtures cannot be launched reliably in the hosted runner. The Windows SDK job skips those callback fixtures plus three upstream Git-for-Windows worktree tests that fail when creating worktree paths containing spaces. The API/server/SDK crates compile on both platforms, the remaining SDK tests run on both, and separate Linux and Windows runtime-smoke jobs exercise the actual pinned executable and backend lifecycle.

## Performance claims and remaining scope

JCode is the runtime engine in this integration, but no fixed speedup is claimed. A reproducible first-pass comparison is provided at `benchmarks/benchmark_jcode_runtime.py`. It compares the legacy Python AgentRunner's single OpenAI-compatible model call with a JCode managed turn using the same provider, model, system prompt, and user prompt; it reports raw samples, median/p95 latency, and median speedup. It intentionally disables tool execution and measures turn latency rather than model quality.

Run it only after configuring the backend with `JCODE_PROVIDER=openai-api`, `JCODE_MODEL`, and an OpenAI API key, and installing the worker package. Set `JCODE_BENCH_TRIALS=30` for 30 measured samples per path. The script performs warmups and then makes billable provider requests; check the provider's pricing before running it. The benchmark does not measure peak RSS, per-session memory, concurrency throughput, or crash recovery; those remain separate release measurements.

The requested 12x target remains an empirical target, not a guaranteed result. No speedup is claimed until the benchmark has been run on the target hardware and its raw output reviewed.

This integration provides concurrent independent agents and live session output. It does not claim feature parity with JCode's native interactive swarm UI or with every original Orchestral autonomous security tool. Any expansion beyond the read-only local-workspace tools requires its own policy/sandbox review and tests.
