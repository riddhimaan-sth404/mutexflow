# orchestral-worker

Python runtime for the Orchestral / Orchestral platform. It provides the
agent execution loop, deterministic guardrails, security tools, the coordinated
swarm, the supervisor, the terminal monitor, and the MCP stdio server.

## What is in here

| Module | Purpose |
|:---|:---|
| `orchestral_worker.agent_runner` | Per-agent ReAct loop with an OpenAI-compatible LLM, or a real deterministic tool plan when no key is configured. |
| `orchestral_worker.guard.scope` | `ScopeGuard` allowlist enforcement for IPs, CIDRs, domains, wildcards, URLs, and IPv6. |
| `orchestral_worker.guard.hitl` | `HITLInterceptor` risk classification and operator approval gate. |
| `orchestral_worker.tools.*` | Native secret scanner, port scanner, HTTP probe, CodeQL wrapper, and the sandboxed terminal tool. |
| `orchestral_worker.sandbox.kali` | Hardened rootless Podman sandbox management. |
| `orchestral_worker.swarm.coordinator` | Recon → DAST → Triage coordinated engagement with a shared blackboard. |
| `orchestral_worker.swarm.independent` | Isolated parallel worker pool for batch tasks. |
| `orchestral_worker.supervisor` | Agent lifecycle, cancellation, token budget, and timeout enforcement. |
| `orchestral_worker.mcp.server` | JSON-RPC 2.0 MCP server over stdio. |
| `orchestral_worker.tui` | Rich terminal monitor with SSE updates and HITL keybindings. |

## Install

```bash
cd orchestral-worker
python3 -m pip install -e .
```

Runtime dependencies are `rich` and `PyYAML`.

## Run

The worker talks to the Rust core over HTTP. Start the core first, then export
the admin key:

```bash
export ORCHESTRAL_API_KEY="<admin key>"
export ORCHESTRAL_CORE_URL="http://127.0.0.1:8088"

# Coordinated engagement
python3 -m orchestral_worker.main --mode coordinated \
  --target 127.0.0.1 --allow 127.0.0.1

# MCP server (stdio)
python3 -m orchestral_worker.mcp.server

# Terminal monitor
python3 -m orchestral_worker.tui --snapshot-only
```

The top-level `bin/orchestral` CLI is the usual entry point and wraps these
commands (`run`, `agent`, `archon`, `lens`, `mcp`, `tartarus`, and others).

## Environment variables

| Variable | Meaning |
|:---|:---|
| `ORCHESTRAL_API_KEY` | Core API key or agent token. Required for core access. |
| `ORCHESTRAL_CORE_URL` | Core base URL (default `http://127.0.0.1:8088`). |
| `ORCHESTRAL_SCOPE` | MCP allowlist, comma separated. |
| `ORCHESTRAL_RECIPES_DIR` | Override the recipes directory when running outside a checkout. |
| `ORCHESTRAL_LLM_API_KEY` / `OPENAI_API_KEY` / `GEMINI_API_KEY` | Enable the LLM loop. |
| `ORCHESTRAL_LLM_BASE_URL` | OpenAI-compatible base URL. |
| `ORCHESTRAL_LLM_MODEL` | Model name. |

## Risk tiers

- `passive`: local file/static inspection. Never blocks.
- `active`: network probes such as port scans and HTTP requests.
- `high_impact`: exploitation, credential attacks, destructive commands.

The CLI defaults to `--auto-approve active`, so active scans run without a
prompt while high-impact actions still require operator approval. Use
`--auto-approve passive` to gate active scans too, or `high_impact` to
auto-approve everything.

## Tests

```bash
python3 -m unittest tests.test_unit_guards -v
python3 -m unittest tests.test_terminal_mcp -v
```

The integration suite expects a running core and a container runtime for the
sandbox test; that test is skipped when no runtime/image is available.
