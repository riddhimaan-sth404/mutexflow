# Security Policy

This document describes MutexFlow's security model for the Orchestral/JCode
integration, how credentials are separated, and what to do if a credential
leaks. MutexFlow is under active development and is not production-hardened.

## Threat model

MutexFlow runs on a single user's machine and assumes the following attackers:

1. **Malicious web content in the browser.** A web page must not be able to
   drive the local backend (DNS rebinding, CSRF, or cross-origin requests).
2. **Other local processes and unprivileged users.** Anything that can reach
   the backend's HTTP port must not be able to control the agent runtime,
   read findings, or approve its own actions.
3. **Untrusted workspace content.** Scanned files, search results, and model
   output are treated as untrusted data. Secrets detected on disk are masked
   before they leave the worker; tool output is never executed.

Out of scope: a fully compromised user account (the attacker already controls
the user's files and credentials) and physical access to an unlocked machine.

## Loopback-only binding

The backend binds to `127.0.0.1` by default (`BIND_ADDR`). Citadel binds to
loopback port `8088` and the local-analysis worker to loopback port `8090`.
Browser CORS is restricted to loopback origins with an explicit method and
header allowlist. Remote binding requires an explicit `BIND_ADDR` override
and must be fronted by an authenticated reverse proxy or firewall; exposing
these endpoints directly to a network is unsupported.

## Local API token (orchestral/JCode routes)

All `/api/v1/orchestral/*` and `/api/v1/jcode/*` routes require a local API
token sent as the `X-MutexFlow-Token` header, in addition to a loopback
`Host` header check. This defends against DNS-rebinding and cross-origin
requests from the browser even when CORS is bypassed.

- On first run the backend generates a 256-bit token from the operating
  system PRNG (with a deterministic fallback for constrained environments)
  and persists it owner-only (0600) at `.mutexflow/backend-token`.
- Set `MUTEXFLOW_API_TOKEN` to supply your own token; it takes precedence.
- The Avalonia desktop client attaches the header automatically from the
  environment variable or the token file. CI supplies its own value.
- Unauthenticated and non-loopback requests are rejected with `401`; this is
  asserted by unit tests and by the managed-runtime end-to-end workflow.

Treat `.mutexflow/backend-token` like a private key: do not commit it, do not
copy it into project directories that get scanned or shared, and delete it if
it may have been disclosed.

## Credential separation

Three distinct credentials exist and must never be conflated:

| Credential | Environment variable | Held by | Purpose |
|---|---|---|---|
| Citadel admin key | `ORCHESTRAL_ADMIN_KEY` | Backend only | Full Citadel control plane (agents, findings, approvals, audit) |
| Worker token | `MUTEXFLOW_WORKER_TOKEN` | Backend and worker | Worker authentication and finding submission, via the backend proxy only |
| Local API token | `MUTEXFLOW_API_TOKEN` | Backend and local UI clients | Desktop/CLI access to the orchestral/JCode routes |

The worker never receives or uses the Citadel admin key. It submits findings
through the backend's authenticated proxy route
(`POST /api/v1/orchestral/findings/submit`), and the backend attaches the
privileged key server-side. The desktop client never sees the admin key.

## Worker boundary

The managed worker exposes only passive, local-workspace secret-pattern
scanning. It rejects absolute paths and path traversal, confines scans to the
configured workspace root, masks every detected value before submission, and
requires its token on every request. Findings contain masked evidence only.

## Dormant offensive tooling

The vendored Orchestral snapshot includes upstream tooling that is offensive
or dual-use: a sandboxed Kali terminal, a native port scanner, an HTTP probe,
a CodeQL analyzer, the Kali container sandbox, and the autonomous swarm
coordinator. In MutexFlow these are **dormant**:

- The worker's tool registry ships with only the passive secret scanner
  registered. The extended tools are imported and registered solely when the
  operator explicitly sets `MUTEXFLOW_ENABLE_EXTENDED_TOOLS=1`.
- The standalone swarm runner entry point refuses to start unless the same
  variable is set.
- The managed MutexFlow worker never enables them, and no MutexFlow workflow
  exposes them.
- The modules are marked as dormant in their source headers.

Any future use of these capabilities requires a documented target
authorization, a sandbox review, Citadel policy evaluation, human approval,
and audit coverage before it ships enabled.

## Secret handling and redaction

File contents and search previews that reach the model are redacted for
common credential formats, including AWS keys, GitHub tokens, Slack tokens,
Google API keys, PEM private-key blocks, bare JWTs, and Google refresh
tokens. Detected secrets are masked in findings. Audit records keep tool
names and path/query lengths, not query text.

## If a credential leaks

1. **Rotate it first** at the provider (GitHub, AWS, Slack, Google, OpenAI,
   Anthropic, OpenRouter, or your identity provider). Rotation limits damage
   faster than any local cleanup.
2. **Purge it from history.** Removing a secret from the latest commit is not
   enough; rewrite history (for example with `git filter-repo` or BFG) and
   force-push, then notify collaborators to re-clone.
3. **Revoke sessions/tokens** where the provider supports it, and check audit
   logs for use you did not perform.
4. **Rotate MutexFlow's own credentials** if affected: delete
   `.mutexflow/backend-token` (a new one is generated on next start), change
   `ORCHESTRAL_ADMIN_KEY`, `ORCHESTRAL_JWT_SECRET`, and
   `MUTEXFLOW_WORKER_TOKEN` in `backend-rust/.env`, and restart the backend.

## Reporting a vulnerability

Please report security issues privately to the maintainer rather than
opening a public issue. Include reproduction steps and affected components.
Do not include real secrets, personal data, or customer data in reports.
