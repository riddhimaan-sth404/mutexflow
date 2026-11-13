# MutexFlow

**A desktop AI workflow assistant with local inference, cloud-provider fallback, and connected-service integrations.**

MutexFlow combines a native desktop interface with a Rust backend to let users run AI-assisted workflows and route results to external tools.

> **Status:** Under active development. The repository currently contains an evolving prototype; features, configuration, and APIs may change.

## Overview

MutexFlow offers three ways to run an AI workflow:

- **Local mode** — run inference through a locally hosted `llama-server` using GGUF models.
- **Cloud Standard mode** — send requests through a configurable sequence of cloud AI providers. The backend tries configured providers in order and returns the first successful response.
- **BYOK mode** — use your own OpenRouter API key from the desktop client.

The desktop client also provides workflow presets, integration configuration, and a local model hub.

## Architecture

| Component | Technology | Responsibility |
|---|---|---|
| Desktop application | C#, .NET 9, Avalonia 11 | User interface, workflow selection, local model management, and integration settings |
| Backend | Rust, Tokio, Axum | HTTP API, cloud-provider cascade, licensing checks, OAuth callbacks, and server-side integration dispatch |
| Optional persistence | Supabase | License records and connected integration tokens |
| Local inference | `llama-server` + GGUF | Runs a model on the user's machine |
| Cloud inference | OpenAI-compatible chat-completions APIs | Provider fallback for Standard mode |

The desktop client communicates with the Rust backend over HTTP. Local inference is exposed through a llama.cpp-compatible endpoint at `http://localhost:8080`.

## Current capabilities

### AI workflows

- Local inference through a locally running `llama-server`.
- Cloud inference through a sequential provider cascade.
- Bring Your Own Key (BYOK) requests through OpenRouter.
- Workflow presets for general assistance, data extraction, and professional drafting.
- Task-complexity selection in the desktop interface.

### Local AI hub

The client includes a model selection and download flow for GGUF models hosted on Hugging Face. The current model list includes:

- Llama 3.1 8B Instruct
- DeepSeek R1 Distill Llama 8B
- Qwen 2.5 Coder 7B Instruct
- Phi-3.5 Mini Instruct
- Mistral 7B Instruct v0.3

You must provide a compatible `llama-server` executable separately; the application code looks for it in its application data binary directory and bundled resource locations.

### Cloud provider cascade

The backend currently defines provider entries for:

- Cerebras
- Groq
- Google Gemini
- SambaNova
- SiliconFlow
- DeepSeek
- Together AI
- Fireworks AI
- Novita AI
- Hugging Face

Only providers with a non-empty API key configured in the backend environment are attempted.

### Integrations

The desktop interface currently lists Slack, GitHub, Google Sheets, Linear, Jira, Notion, Confluence, HubSpot, and Salesforce.

**Implementation note:** the current backend dispatch service contains concrete handlers for Slack, GitHub, and Google Sheets. Other entries are represented in the UI/status model, but should not be assumed to have complete end-to-end dispatch support yet. The local direct-dispatch path currently handles Slack and GitHub.

### Licensing and connected accounts

The backend includes license verification and OAuth connect/callback routes. Supabase configuration is optional for standalone development, but database-backed license and integration-token operations require a correctly configured Supabase project.

## Repository layout

```text
mutexflow/
├── backend-rust/
│   ├── src/
│   │   ├── config/       # Provider, database, and OAuth configuration
│   │   ├── models/       # API request and response models
│   │   ├── routes/       # Axum HTTP routes
│   │   └── services/     # AI cascade and integration dispatch
│   ├── tests/            # Backend API tests
│   ├── .env.example      # Backend environment template
│   └── Cargo.toml
├── desktop-avalonia/
│   ├── Models/
│   ├── Services/         # Backend client, local AI, OAuth, and storage
│   ├── ViewModels/
│   ├── Views/
│   └── MutexFlow.Desktop.csproj
├── run.bat               # Windows development launcher
└── .gitignore
```

## Requirements

For the current development setup:

- Git
- Rust toolchain (including Cargo)
- .NET 9 SDK
- A supported desktop environment for Avalonia
- Network access for downloading models or using cloud providers
- Optional: a compatible `llama-server` executable and a GGUF model for local inference
- Optional: Supabase project credentials for database-backed licensing and integrations
- Optional: API keys for the cloud providers and credentials for the integrations you configure

## Getting started

### 1. Clone the repository

```bash
git clone https://github.com/riddhimaan-sth404/mutexflow.git
cd mutexflow
```

### 2. Configure the backend

Create a local environment file from the template:

**Windows (PowerShell):**

```powershell
Copy-Item backend-rust/.env.example backend-rust/.env
```

**Linux/macOS:**

```bash
cp backend-rust/.env.example backend-rust/.env
```

Edit `backend-rust/.env` and provide the credentials needed for the features you intend to use.

At minimum, the backend uses port `3001` by default. Cloud Standard mode needs at least one configured cloud-provider API key. Supabase variables are only needed for the database-backed features.

Never commit `.env` files, API keys, OAuth secrets, or service-role credentials.

### 3. Run the application on Windows

The repository includes a Windows launcher:

```bat
run.bat
```

It starts the Rust backend with `cargo run`, then launches the Avalonia desktop client with `dotnet run`.

You can also start the components separately:

**Terminal 1 — backend:**

```powershell
cd backend-rust
cargo run
```

**Terminal 2 — desktop client:**

```powershell
cd desktop-avalonia
dotnet run
```

The launcher is Windows-specific. On other operating systems, start the backend and desktop project separately using the equivalent shell commands; platform-specific packaging and launch instructions are still a work in progress.

### 4. Enable local inference (optional)

1. Obtain a compatible `llama-server` executable for your operating system.
2. Place it in the MutexFlow application data binary directory or a supported bundled `resources/bin` location.
3. Open the Local AI section in MutexFlow and select a GGUF model.
4. Download the model and start a local workflow.

The client expects the local server's OpenAI-compatible API at:

```text
http://localhost:8080/v1/chat/completions
```

The model files are stored under the MutexFlow application data directory.

## Backend API

The current Axum server exposes these routes:

| Method | Route | Purpose |
|---|---|---|
| `POST` | `/api/v1/auth/verify-license` | Verify a license against the supplied machine identifier |
| `GET` | `/api/v1/auth/:provider/connect` | Begin an OAuth connection flow |
| `GET` | `/api/v1/auth/:provider/callback` | Handle an OAuth provider callback |
| `POST` | `/api/v1/agent/process-standard` | Run a Standard cloud-cascade workflow |
| `POST` | `/api/v1/integrations/dispatch` | Dispatch a result to selected integrations |
| `GET` | `/api/v1/integrations/status` | Retrieve integration connection status |

The default backend address is `http://localhost:3001`. The port can be changed with the `PORT` environment variable.

## Development

### Build and test the Rust backend

```bash
cd backend-rust
cargo build
cargo test
```

### Build the desktop client

```bash
cd desktop-avalonia
dotnet restore
dotnet build
```

## Configuration reference

The backend template at `backend-rust/.env.example` documents the current environment variables, including:

| Variable | Purpose |
|---|---|
| `PORT` | Backend listening port (defaults to `3001`) |
| `BACKEND_URL` | Public/base URL used to construct OAuth callback URLs |
| `SUPABASE_URL` | Supabase project URL |
| `SUPABASE_SERVICE_KEY` | Supabase service-role credential |
| `CEREBRAS_API_KEY`, `GROQ_API_KEY`, etc. | Cloud provider credentials |
| `SLACK_CLIENT_ID`, `SLACK_CLIENT_SECRET` | Slack OAuth application credentials |
| `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET` | GitHub OAuth application credentials |
| `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET` | Google OAuth application credentials |
| `GOOGLE_SHEET_ID` | Target spreadsheet for Google Sheets dispatch |
| `GITHUB_TARGET_REPO` | Default repository used by backend GitHub dispatch |

Use the exact variable names in `.env.example`; do not place secrets in source code.

## Security considerations

MutexFlow is under development and should not be treated as production-hardened.

- The current backend binds to `0.0.0.0` and configures permissive CORS. Do not expose it to an untrusted network without first restricting network access, origins, and authentication.
- Keep Supabase service-role keys, provider API keys, OAuth client secrets, and integration tokens private.
- In the current desktop storage implementation, Windows secrets use DPAPI for the current user. The non-Windows fallback writes secret values as ordinary files; do not assume equivalent at-rest protection on Linux or macOS.
- Review license verification and offline behavior before relying on licensing as an enforcement boundary.
- Do not send sensitive prompts or data to a cloud provider unless you have reviewed that provider's data-handling terms.

Security reports should be handled privately and responsibly. Avoid publishing details of an unpatched vulnerability.

## Roadmap

The following are areas for continued development, not promises of completed functionality:

- Complete and test all advertised integration adapters.
- Improve cross-platform packaging and first-run setup.
- Strengthen credential storage on non-Windows platforms.
- Tighten backend CORS, binding, and authentication defaults.
- Expand automated tests for AI routing, OAuth, licensing, and integration delivery.
- Document supported provider/model combinations and failure behavior.

## Contributing

Contributions, bug reports, and focused suggestions are welcome.

1. Open an issue describing the problem or proposed change.
2. Keep changes scoped and include tests where practical.
3. Do not include secrets, personal tokens, or real customer data in issues or pull requests.
4. Clearly identify incomplete or experimental functionality.

## License

A license file was not present in the repository root when this README was prepared. Until a license is added, reuse, redistribution, and contribution terms are not clearly defined. Add an explicit `LICENSE` file before presenting the project as open source.

---

**MutexFlow** — local inference when you want it, cloud fallback when you need it, and workflows that connect AI output to the tools you use.



### Managed runtime setup

The MutexFlow backend can manage the Orchestral Citadel and the restricted local-analysis worker as child processes. Runtime autostart is enabled only when a non-empty `ORCHESTRAL_ADMIN_KEY` is configured (and `ORCHESTRAL_AUTOSTART` is not disabled).

1. Copy `backend-rust/.env.example` to `backend-rust/.env`.
2. Generate a private random admin key locally (for example, with `openssl rand -hex 32`) and set it as `ORCHESTRAL_ADMIN_KEY`. Do not commit the real `.env` file.
3. Set a stable, random `ORCHESTRAL_JWT_SECRET` for agent-token continuity across Citadel restarts.
4. Install Rust/Cargo and Python 3.10 or newer. The restricted worker uses the Python standard library plus the vendored Orchestral worker modules.
5. Start MutexFlow with `run.bat` on Windows or launch the Rust backend from `backend-rust/` on other platforms.

The backend binds to loopback by default (`BIND_ADDR=127.0.0.1`), starts Citadel on loopback port 8088 and the local worker on loopback port 8090, and restricts browser CORS origins to loopback. Remote binding requires an explicit `BIND_ADDR` override and should be protected by an authenticated reverse proxy/firewall. It shuts both down when the backend receives Ctrl+C. Set `ORCHESTRAL_CORE_BIN` to use a prebuilt Citadel executable instead of `cargo run`; set `PYTHON` to override the Python executable.

### Orchestral dashboard and supported workflow

The desktop's **Orchestral** section displays runtime health, registered agents, findings, pending human approvals, and recent audit events. Local analysis jobs support cooperative cancellation. Approval and denial actions are routed through the MutexFlow backend; the Citadel admin key is never sent to the desktop.

The first executable workflow is **passive local workspace analysis**. Enter a relative directory beneath the configured MutexFlow workspace root (the repository root by default). The worker runs only the native secret-pattern scanner, masks any detected values, and records masked findings in Citadel. It rejects absolute paths and traversal outside the workspace root. It does not expose the autonomous network-scanning, terminal, or exploit-capable tools through this desktop workflow.

**Integration boundary:** the imported VigilEngine autonomous swarm is preserved as source but is not launched by MutexFlow. General-purpose agent spawning, arbitrary tool dispatch, streaming event UI, and end-to-end cancellation remain future work; no claim is made that the full standalone Orchestral agent stack is already integrated.


## Orchestral runtime integration (in progress)

MutexFlow contains an imported snapshot of [Orchestral](https://github.com/riddhimaan-sth404/orchestral) under `orchestral-runtime/`. The backend manages the Citadel sidecar and a dedicated, restricted local-analysis worker when runtime configuration is present.

Backend endpoints are grouped under `/api/v1/orchestral`:

- `GET /status` and `GET /health` — component readiness and process status.
- `GET /agents`, `GET /findings`, `GET /approvals`, and `GET /audit` — read Citadel state.
- `POST /approvals/{id}/approve` and `POST /approvals/{id}/deny` — operator approval decisions.
- `POST /jobs/local-analysis`, `GET /jobs/{id}`, and `POST /jobs/{id}/cancel` — start, observe, and cooperatively cancel passive local workspace analysis.

The backend keeps the Citadel admin key server-side and binds to loopback by default. The worker accepts only relative workspace paths confined to the configured repository root, runs the passive secret-pattern scanner, masks evidence, and records findings in Citadel. The imported autonomous VigilEngine swarm is not exposed through this adapter. The managed-runtime CI smoke test exercises backend-managed startup, local analysis, approval grant/denial, audit retrieval, and child-process shutdown.

Existing MutexFlow inference, OAuth, licensing, integrations, and desktop behavior are preserved. See [ORCHESTRAL_MERGE_PLAN.md](ORCHESTRAL_MERGE_PLAN.md) for the current boundary and remaining work.

### JCode concurrent agent runtime (experimental)

The same backend can optionally manage a pinned [JCode](https://github.com/1jehuang/jcode) Harness instance. Set `JCODE_AUTOSTART=true` and `JCODE_BINARY` to the installed JCode executable in `backend-rust/.env`. JCode is launched with a clean environment, a dedicated `.mutexflow/jcode` state home, and no inherited JCode/provider login files. Its API is connected through the upstream Rust SDK.

The Avalonia Orchestral dashboard exposes session creation, individual turns, cancellation, and concurrent task batches (up to eight sessions/turns). Backend endpoints are grouped under `/api/v1/jcode`: `GET /health`, `GET|POST /sessions`, `POST /sessions/{id}/run`, `POST /sessions/{id}/cancel`, and `POST /swarm` (one task per entry in `tasks`). Each new session starts with an empty built-in tool allowlist. This deliberately prevents JCode from running shell, modifying files, or making network calls outside Citadel's policy and approval path. The current adapter provides concurrent model-agent sessions, not yet the full autonomous JCode swarm with tool-using workers.

JCode requires a provider configured inside its isolated state home before turns can produce model output. The runtime smoke workflow builds the pinned JCode executable and verifies isolated startup, session creation, tool-disable configuration, session listing, and cancellation without requiring provider credentials. Performance claims remain unverified; see [JCODE_INTEGRATION.md](JCODE_INTEGRATION.md) for the security and benchmark gates.
