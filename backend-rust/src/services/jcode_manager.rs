use jcode_sdk::{ConnectOptions, JcodeClient, RunOptions, ToolConfiguration};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use tracing::{info, warn};

const MAX_PROMPT_BYTES: usize = 64 * 1024;
const MAX_CONCURRENT_TURNS: usize = 8;

struct JcodeRuntime {
    client: JcodeClient,
    child: Mutex<Child>,
    home: PathBuf,
}

impl Drop for JcodeRuntime {
    fn drop(&mut self) {
        stop_registered_daemon(&self.home);
        if let Ok(child) = self.child.get_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct JcodeStatus {
    pub enabled: bool,
    pub connected: bool,
    pub binary: String,
    pub max_concurrent_turns: usize,
    pub message: String,
}

pub struct JcodeManager {
    root: PathBuf,
    binary: PathBuf,
    enabled: bool,
    runtime: Mutex<Option<Arc<JcodeRuntime>>>,
    turns: Arc<Semaphore>,
}

impl JcodeManager {
    pub fn new(root: PathBuf) -> Self {
        let binary = std::env::var_os("JCODE_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "jcode.exe" } else { "jcode" }));
        let enabled = std::env::var("JCODE_AUTOSTART")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false);
        Self {
            root,
            binary,
            enabled,
            runtime: Mutex::new(None),
            turns: Arc::new(Semaphore::new(MAX_CONCURRENT_TURNS)),
        }
    }

    pub fn status(&self) -> JcodeStatus {
        let connected = self.runtime.lock().map(|slot| {
            slot.as_ref().map(|runtime| runtime.client.ping().is_ok()).unwrap_or(false)
        }).unwrap_or(false);
        JcodeStatus {
            enabled: self.enabled,
            connected,
            binary: self.binary.display().to_string(),
            max_concurrent_turns: MAX_CONCURRENT_TURNS,
            message: if !self.enabled {
                "JCode is disabled. Set JCODE_AUTOSTART=true and configure JCODE_BINARY to enable the isolated runtime.".into()
            } else if connected {
                "JCode Harness is connected. Sessions use an isolated home and start with built-in tools disabled.".into()
            } else {
                "JCode is enabled but not connected. Check that the configured JCode binary is installed and compatible.".into()
            },
        }
    }

    pub fn start(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        if self.runtime.lock().map_err(|_| "JCode runtime lock poisoned")?.is_some() {
            return Ok(());
        }

        let home = self.root.join(".mutexflow").join("jcode");
        let runtime_dir = home.join("run");
        std::fs::create_dir_all(&runtime_dir).map_err(|e| format!("create isolated JCode home: {e}"))?;
        set_private_dir(&home)?;
        set_private_dir(&runtime_dir)?;
        let socket = runtime_dir.join("jcode-api.sock");

        let mut command = Command::new(&self.binary);
        command
            .env_clear()
            .args(["--no-update", "api-bridge", "--api-socket"])
            .arg(&socket)
            .current_dir(&self.root)
            .env("JCODE_HOME", &home)
            .env("JCODE_RUNTIME_DIR", &runtime_dir)
            .env("JCODE_API_SOCKET", &socket)
            .env("JCODE_SOCKET", runtime_dir.join("jcode.sock"))
            .env("JCODE_NO_AUTO_UPDATE", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        // Only pass variables required to locate and launch the executable.
        if let Some(path) = std::env::var_os("PATH") {
            command.env("PATH", path);
        }
        #[cfg(windows)]
        for name in ["SYSTEMROOT", "WINDIR", "TEMP", "TMP"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        #[cfg(unix)]
        if let Some(value) = std::env::var_os("HOME") {
            // JCode's own state is directed to JCODE_HOME; HOME is retained only
            // for platform libraries that require a home directory.
            let _ = value;
            command.env("HOME", &home);
        }

        let mut child = command.spawn().map_err(|e| format!("start isolated JCode Harness: {e}"))?;
        let deadline = Instant::now() + Duration::from_secs(45);
        let client = loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    stop_registered_daemon(&home);
                    return Err(format!("JCode Harness exited during startup ({status})"));
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    stop_registered_daemon(&home);
                    return Err(format!("check JCode process: {error}"));
                }
                Ok(None) => {}
            }
            let options = ConnectOptions {
                socket_path: Some(socket.clone()),
                client_name: "MutexFlow/Orchestral".into(),
                request_timeout: Some(Duration::from_secs(3)),
                ensure_runtime: false,
                supports_user_response: false,
            };
            if let Ok(client) = JcodeClient::connect(options) {
                break client;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                stop_registered_daemon(&home);
                return Err("JCode Harness did not become ready within 45 seconds".into());
            }
            std::thread::sleep(Duration::from_millis(250));
        };

        let runtime = Arc::new(JcodeRuntime { client, child: Mutex::new(child), home });
        *self.runtime.lock().map_err(|_| "JCode runtime lock poisoned")? = Some(runtime);
        info!("[JCode] Isolated Harness started; inherited credentials are disabled.");
        Ok(())
    }

    fn client(&self) -> Result<Arc<JcodeRuntime>, String> {
        self.runtime.lock()
            .map_err(|_| "JCode runtime lock poisoned".to_string())?
            .clone()
            .ok_or_else(|| "JCode is not running. Enable JCODE_AUTOSTART and restart MutexFlow.".into())
    }

    pub async fn create_session(&self, workspace: &str) -> Result<Value, String> {
        if workspace.trim().is_empty() || workspace.len() > 1024 {
            return Err("workspace must be a non-empty relative path of at most 1024 bytes".into());
        }
        let root = self.root.clone();
        let requested = workspace.to_string();
        let runtime = self.client()?;
        tokio::task::spawn_blocking(move || {
            let root = root.canonicalize().map_err(|e| format!("resolve workspace root: {e}"))?;
            let candidate = root.join(requested);
            let workspace = candidate.canonicalize().map_err(|e| format!("resolve workspace: {e}"))?;
            if !workspace.starts_with(&root) || !workspace.is_dir() {
                return Err("workspace must resolve to a directory inside the MutexFlow workspace root".into());
            }
            let session = runtime.client.create_session(Some(workspace.to_string_lossy().to_string()))
                .map_err(|e| format!("create JCode session: {e}"))?;
            // Fail closed: no shell, filesystem mutation, network, MCP, or other
            // built-in tools until each tool has a Citadel-mediated adapter.
            runtime.client.configure_tools(&session.session_id, ToolConfiguration {
                enabled: Some(Vec::new()),
                ..ToolConfiguration::default()
            }).map_err(|e| format!("disable JCode built-in tools: {e}"))?;
            Ok(json!({ "session_id": session.session_id, "status": "created", "tools_enabled": 0 }))
        }).await.map_err(|e| format!("JCode session task failed: {e}"))?
    }

    pub async fn run(&self, session_id: &str, prompt: &str) -> Result<Value, String> {
        if session_id.trim().is_empty() || session_id.len() > 128 {
            return Err("invalid JCode session identifier".into());
        }
        if prompt.trim().is_empty() || prompt.len() > MAX_PROMPT_BYTES {
            return Err(format!("prompt must be non-empty and at most {MAX_PROMPT_BYTES} bytes"));
        }
        let permit = self.turns.clone().acquire_owned().await
            .map_err(|_| "JCode concurrency gate is closed".to_string())?;
        let runtime = self.client()?;
        let session_id = session_id.to_string();
        let prompt = prompt.to_string();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let known = runtime.client.list_sessions().map_err(|e| format!("list JCode sessions: {e}"))?;
            if !known.iter().any(|session| session.session_id == session_id) {
                return Err("JCode session was not found".into());
            }
            let result = runtime.client.run(&session_id, &prompt, RunOptions::default())
                .map_err(|e| format!("JCode turn failed: {e}"))?;
            Ok(json!({
                "session_id": session_id,
                "text": result.final_text,
                "stop_reason": result.stop_reason.map(|v| format!("{v:?}")),
                "tool_calls": result.tool_calls.len()
            }))
        }).await.map_err(|e| format!("JCode turn task failed: {e}"))?
    }

    pub async fn cancel(&self, session_id: &str) -> Result<Value, String> {
        let runtime = self.client()?;
        let session_id = session_id.to_string();
        tokio::task::spawn_blocking(move || {
            runtime.client.cancel(&session_id).map_err(|e| format!("cancel JCode session: {e}"))?;
            Ok(json!({ "session_id": session_id, "status": "cancellation_requested" }))
        }).await.map_err(|e| format!("JCode cancellation task failed: {e}"))?
    }


    pub async fn run_batch(&self, workspace: &str, prompts: Vec<String>) -> Result<Value, String> {
        if prompts.is_empty() || prompts.len() > MAX_CONCURRENT_TURNS {
            return Err(format!("batch must contain between 1 and {MAX_CONCURRENT_TURNS} agent tasks"));
        }
        if prompts.iter().any(|prompt| prompt.trim().is_empty() || prompt.len() > MAX_PROMPT_BYTES) {
            return Err(format!("every task must be non-empty and at most {MAX_PROMPT_BYTES} bytes"));
        }
        let mut pending = futures_util::stream::FuturesUnordered::new();
        let mut session_ids = Vec::with_capacity(prompts.len());
        for prompt in prompts {
            let created = self.create_session(workspace).await?;
            let session_id = created["session_id"].as_str()
                .ok_or_else(|| "JCode returned a session without an identifier".to_string())?
                .to_string();
            session_ids.push(session_id.clone());
            pending.push(async move {
                let result = self.run(&session_id, &prompt).await;
                (session_id, result)
            });
        }
        let mut results = Vec::with_capacity(session_ids.len());
        while let Some((session_id, result)) = futures_util::StreamExt::next(&mut pending).await {
            match result {
                Ok(value) => results.push(value),
                Err(error) => results.push(json!({ "session_id": session_id, "error": error })),
            }
        }
        Ok(json!({ "agent_count": results.len(), "results": results }))
    }

    pub async fn sessions(&self) -> Result<Value, String> {
        let runtime = self.client()?;
        tokio::task::spawn_blocking(move || {
            let sessions = runtime.client.list_sessions().map_err(|e| format!("list JCode sessions: {e}"))?;
            serde_json::to_value(sessions).map_err(|e| format!("serialize JCode sessions: {e}"))
        }).await.map_err(|e| format!("JCode session listing failed: {e}"))?
    }
}


fn stop_registered_daemon(home: &Path) {
    let runtime_dir = home.join("run");
    let expected_socket = runtime_dir.join("jcode.sock");
    let registry_path = home.join("servers.json");
    let Ok(raw) = std::fs::read(registry_path) else { return; };
    let Ok(registry) = serde_json::from_slice::<Value>(&raw) else { return; };
    let Some(entries) = registry.as_object() else { return; };
    for entry in entries.values() {
        let Some(socket) = entry.get("socket").and_then(Value::as_str) else { continue; };
        let Some(pid) = entry.get("pid").and_then(Value::as_i64) else { continue; };
        if Path::new(socket) != expected_socket || pid <= 1 || pid > i32::MAX as i64 {
            continue;
        }
        #[cfg(unix)]
        {
            let pid = pid as i32;
            // The SDK's own shutdown routine targets the registered daemon's
            // process group, falling back to the PID. Mirror that behavior only
            // after matching the socket inside our private JCODE_HOME.
            unsafe {
                if libc::kill(-pid, libc::SIGTERM) != 0 {
                    libc::kill(pid, libc::SIGTERM);
                }
            }
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline && process_exists(pid) {
                std::thread::sleep(Duration::from_millis(50));
            }
            if process_exists(pid) {
                unsafe {
                    if libc::kill(-pid, libc::SIGKILL) != 0 {
                        libc::kill(pid, libc::SIGKILL);
                    }
                }
            }
        }
        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

#[cfg(unix)]
fn process_exists(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

#[cfg(unix)]
fn set_private_dir(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("restrict JCode state directory: {e}"))
}

#[cfg(not(unix))]
fn set_private_dir(_path: &Path) -> Result<(), String> {
    Ok(())
}
