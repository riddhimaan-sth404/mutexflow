use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::info;

struct RuntimeProcesses {
    core: Option<Child>,
    worker: Option<Child>,
}

#[derive(Clone, Debug, Serialize)]
pub struct OrchestralRuntimeStatus {
    pub enabled: bool,
    pub managed: bool,
    pub core_healthy: bool,
    pub worker_healthy: bool,
    pub core_url: String,
    pub worker_url: String,
    pub message: String,
}

pub struct OrchestralManager {
    processes: Mutex<RuntimeProcesses>,
    client: reqwest::Client,
    root: PathBuf,
    admin_key: Option<String>,
    core_url: String,
    worker_url: String,
    enabled: bool,
}

impl OrchestralManager {
    pub fn new(root: PathBuf) -> Self {
        let admin_key = std::env::var("ORCHESTRAL_ADMIN_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let enabled = admin_key.is_some()
            && std::env::var("ORCHESTRAL_AUTOSTART")
                .map(|value| value.eq_ignore_ascii_case("true") || value == "1")
                .unwrap_or(true);
        let core_url = loopback_base_url(
            &std::env::var("ORCHESTRAL_CORE_URL").unwrap_or_default(),
            "http://127.0.0.1:8088",
        );
        let worker_url = loopback_base_url(
            &std::env::var("ORCHESTRAL_WORKER_URL").unwrap_or_default(),
            "http://127.0.0.1:8090",
        );

        Self {
            processes: Mutex::new(RuntimeProcesses {
                core: None,
                worker: None,
            }),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("reqwest client configuration is valid"),
            root,
            admin_key,
            core_url,
            worker_url,
            enabled,
        }
    }

    pub async fn start(&self) -> Result<(), String> {
        if !self.enabled {
            info!("[Orchestral] Autostart disabled; set ORCHESTRAL_ADMIN_KEY and ORCHESTRAL_AUTOSTART=true to enable.");
            return Ok(());
        }

        let mut processes = self.processes.lock().await;
        if processes.core.is_some() && processes.worker.is_some() {
            return Ok(());
        }

        let data_dir = self.root.join(".mutexflow").join("orchestral");
        std::fs::create_dir_all(&data_dir).map_err(|e| format!("create Orchestral data directory: {e}"))?;

        let admin_key = self.admin_key.as_deref().unwrap_or_default();
        let core_dir = self.root.join("orchestral-runtime").join("core");
        let core_manifest = core_dir.join("Cargo.toml");
        let configured_core_bin = std::env::var("ORCHESTRAL_CORE_BIN")
            .ok()
            .filter(|v| !v.trim().is_empty());

        let core_binary = if let Some(binary) = configured_core_bin {
            PathBuf::from(binary)
        } else {
            let binary_name = if cfg!(windows) { "orchestral-core.exe" } else { "orchestral-core" };
            let binary = core_dir.join("target").join("debug").join(binary_name);
            let mut build_command = Command::new("cargo");
            build_command.kill_on_drop(true);
            build_command.env_clear();
            inherit_safe_runtime_environment(&mut build_command);
            for name in ["CARGO_HOME", "RUSTUP_HOME", "CARGO_TARGET_DIR", "RUSTUP_TOOLCHAIN", "CARGO_NET_OFFLINE", "CARGO_HTTP_PROXY", "CARGO_HTTP_TIMEOUT", "CARGO_REGISTRIES_CRATES_IO_PROTOCOL", "RUSTFLAGS", "RUSTC_WRAPPER"] {
                if let Some(value) = std::env::var_os(name) {
                    build_command.env(name, value);
                }
            }
            let build = build_command
                .args(["build", "--manifest-path"])
                .arg(&core_manifest)
                .current_dir(&self.root)
                .output();
            match tokio::time::timeout(Duration::from_secs(180), build).await {
                Ok(Ok(output)) if output.status.success() => {}
                Ok(Ok(output)) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let tail = stderr.lines().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
                    return Err(format!("build Orchestral Citadel failed: {tail}"));
                }
                Ok(Err(error)) => return Err(format!("run Cargo to build Orchestral Citadel: {error}")),
                Err(_) => return Err("Orchestral Citadel build timed out after 180 seconds".to_string()),
            }
            binary
        };

        let mut core_command = Command::new(core_binary);
        core_command.kill_on_drop(true);
        core_command.env_clear();
        inherit_safe_runtime_environment(&mut core_command);
        core_command
            .current_dir(&self.root)
            .env("PORT", port_from_url(&self.core_url).unwrap_or(8088).to_string())
            .env("ORCHESTRAL_BIND_ADDR", "127.0.0.1")
            .env("ORCHESTRAL_ADMIN_KEY", admin_key)
            .env("ORCHESTRAL_DB_PATH", data_dir.join("citadel.sqlite").to_string_lossy().to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        if let Ok(secret) = std::env::var("ORCHESTRAL_JWT_SECRET") {
            if !secret.trim().is_empty() {
                core_command.env("ORCHESTRAL_JWT_SECRET", secret);
            }
        }

        let mut core = core_command.spawn()
            .map_err(|e| format!("start Orchestral Citadel (check Cargo or ORCHESTRAL_CORE_BIN): {e}"))?;

        if !self.wait_for_health(&self.core_url, Duration::from_secs(180), &mut core).await {
            let _ = core.kill().await;
            let _ = core.wait().await;
            return Err("Orchestral Citadel did not become healthy within 180 seconds".to_string());
        }

        let python = std::env::var("PYTHON").ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| {
            if cfg!(windows) { "python".to_string() } else { "python3".to_string() }
        });
        let worker_dir = self.root.join("orchestral-runtime").join("worker");
        let mut worker_command = Command::new(python);
        worker_command.kill_on_drop(true);
        worker_command.env_clear();
        inherit_safe_runtime_environment(&mut worker_command);
        worker_command
            .args(["-m", "orchestral_worker.service"])
            .current_dir(&worker_dir)
            .env("ORCHESTRAL_WORKER_PORT", port_from_url(&self.worker_url).unwrap_or(8090).to_string())
            .env("ORCHESTRAL_CORE_URL", &self.core_url)
            .env("ORCHESTRAL_ADMIN_KEY", admin_key)
            .env("MUTEXFLOW_WORKSPACE_ROOT", &self.root)
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let mut worker = match worker_command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = core.kill().await;
                let _ = core.wait().await;
                return Err(format!("start Orchestral local worker (check Python): {error}"));
            }
        };

        if !self.wait_for_health(&self.worker_url, Duration::from_secs(10), &mut worker).await {
            let _ = worker.kill().await;
            let _ = worker.wait().await;
            let _ = core.kill().await;
            let _ = core.wait().await;
            return Err("Orchestral local worker did not become healthy within 10 seconds".to_string());
        }

        info!("[Orchestral] Citadel and restricted local-analysis worker are ready.");
        processes.core = Some(core);
        processes.worker = Some(worker);
        Ok(())
    }

    async fn wait_for_health(&self, base_url: &str, timeout: Duration, child: &mut Child) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        while tokio::time::Instant::now() < deadline {
            if matches!(child.try_wait(), Ok(Some(_)) | Err(_)) {
                return false;
            }
            if let Ok(response) = self.client.get(format!("{}/health", base_url)).send().await {
                if response.status().is_success() {
                    return true;
                }
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        false
    }

    pub async fn status(&self) -> OrchestralRuntimeStatus {
        let (core_managed, worker_managed) = {
            let mut processes = self.processes.lock().await;
            let core_alive = child_alive(&mut processes.core);
            let worker_alive = child_alive(&mut processes.worker);
            (core_alive, worker_alive)
        };

        let core_healthy = self.health(&self.core_url).await;
        let worker_healthy = self.health(&self.worker_url).await;
        let managed = core_managed && worker_managed;
        let message = if self.admin_key.is_none() {
            "Runtime is not configured. Set ORCHESTRAL_ADMIN_KEY to enable it.".to_string()
        } else if !self.enabled && core_healthy && worker_healthy {
            "Runtime endpoints are reachable; autostart is disabled and the services are externally managed.".to_string()
        } else if !self.enabled {
            "Runtime autostart is disabled. Set ORCHESTRAL_AUTOSTART=true to manage the sidecars.".to_string()
        } else if core_healthy && worker_healthy && managed {
            "Citadel and the restricted local-analysis worker are running.".to_string()
        } else if core_healthy && worker_healthy {
            "Runtime endpoints are reachable, but one or both processes are externally managed.".to_string()
        } else {
            "One or more Orchestral runtime components are unavailable.".to_string()
        };

        OrchestralRuntimeStatus {
            enabled: self.enabled,
            managed,
            core_healthy,
            worker_healthy,
            core_url: self.core_url.clone(),
            worker_url: self.worker_url.clone(),
            message,
        }
    }

    async fn health(&self, base_url: &str) -> bool {
        self.client.get(format!("{}/health", base_url))
            .send().await.map(|r| r.status().is_success()).unwrap_or(false)
    }

    fn core_request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let key = self.admin_key.as_deref().unwrap_or_default();
        self.client.request(method, format!("{}{}", self.core_url, path))
            .header("X-Orchestral-API-Key", key)
    }

    pub async fn core_get(&self, path: &str) -> Result<Value, String> {
        let response = self.core_request(reqwest::Method::GET, path).send().await
            .map_err(|e| format!("Orchestral Citadel request failed: {e}"))?;
        let status = response.status();
        let body = response.json::<Value>().await
            .map_err(|e| format!("Invalid Orchestral Citadel response: {e}"))?;
        if !status.is_success() {
            return Err(format!("Orchestral GET {} returned HTTP {}: {}", path, status, body.get("error").and_then(Value::as_str).unwrap_or("request rejected")));
        }
        Ok(body)
    }

    pub async fn core_post_json(&self, path: &str, payload: &Value) -> Result<Value, String> {
        let response = self.core_request(reqwest::Method::POST, path).json(payload).send().await
            .map_err(|e| format!("Orchestral Citadel request failed: {e}"))?;
        let status = response.status();
        let body = response.json::<Value>().await.unwrap_or_else(|_| json!({}));
        if !status.is_success() {
            return Err(format!("Orchestral POST {} returned HTTP {}: {}", path, status, body.get("error").and_then(Value::as_str).unwrap_or("request rejected")));
        }
        Ok(body)
    }

    pub async fn record_audit(&self, actor: &str, action: &str, details: &str) -> Result<(), String> {
        self.core_post_json("/api/v1/audit/events", &json!({
            "actor": actor,
            "action": action,
            "details": details
        })).await.map(|_| ())
    }

    pub async fn core_post_empty(&self, path: &str) -> Result<Value, String> {
        let response = self.core_request(reqwest::Method::POST, path).send().await
            .map_err(|e| format!("Orchestral Citadel request failed: {e}"))?;
        let status = response.status();
        let body = response.json::<Value>().await.unwrap_or_else(|_| json!({}));
        if !status.is_success() {
            return Err(format!("Orchestral POST {} returned HTTP {}: {}", path, status, body.get("error").and_then(Value::as_str).unwrap_or("request rejected")));
        }
        Ok(body)
    }

    pub async fn start_local_analysis(&self, workspace: &str) -> Result<Value, String> {
        if !self.health(&self.worker_url).await || !self.health(&self.core_url).await {
            return Err("Orchestral runtime is not healthy".to_string());
        }
        let key = self.admin_key.as_deref().unwrap_or_default();
        let response = self.client.post(format!("{}/v1/jobs/local-analysis", self.worker_url))
            .header("X-MutexFlow-Worker-Token", key)
            .json(&json!({ "workspace": workspace }))
            .send().await.map_err(|e| format!("Local analysis worker request failed: {e}"))?;
        let status = response.status();
        let body = response.json::<Value>().await.unwrap_or_else(|_| json!({}));
        if !status.is_success() {
            return Err(body.get("error").and_then(Value::as_str).unwrap_or("Worker rejected the analysis request").to_string());
        }
        Ok(body)
    }

    pub async fn get_local_analysis_job(&self, job_id: &str) -> Result<Value, String> {
        if !is_safe_job_id(job_id) {
            return Err("Invalid job identifier".to_string());
        }
        let key = self.admin_key.as_deref().unwrap_or_default();
        let response = self.client.get(format!("{}/v1/jobs/{}", self.worker_url, job_id))
            .header("X-MutexFlow-Worker-Token", key)
            .send().await.map_err(|e| format!("Local analysis worker request failed: {e}"))?;
        let status = response.status();
        let body = response.json::<Value>().await.unwrap_or_else(|_| json!({}));
        if !status.is_success() {
            return Err(body.get("error").and_then(Value::as_str).unwrap_or("Worker request rejected").to_string());
        }
        Ok(body)
    }

    pub async fn cancel_local_analysis_job(&self, job_id: &str) -> Result<Value, String> {
        if !is_safe_job_id(job_id) {
            return Err("Invalid job identifier".to_string());
        }
        let key = self.admin_key.as_deref().unwrap_or_default();
        let response = self.client
            .post(format!("{}/v1/jobs/{}/cancel", self.worker_url, job_id))
            .header("X-MutexFlow-Worker-Token", key)
            .send().await
            .map_err(|e| format!("Local analysis cancellation failed: {e}"))?;
        let status = response.status();
        let body = response.json::<Value>().await.unwrap_or_else(|_| json!({}));
        if !status.is_success() {
            return Err(body.get("error").and_then(Value::as_str).unwrap_or("Worker rejected cancellation").to_string());
        }
        Ok(body)
    }

    pub async fn shutdown(&self) {
        let mut processes = self.processes.lock().await;
        if let Some(mut worker) = processes.worker.take() {
            let _ = worker.kill().await;
            let _ = worker.wait().await;
        }
        if let Some(mut core) = processes.core.take() {
            let _ = core.kill().await;
            let _ = core.wait().await;
        }
    }
}

fn child_alive(child: &mut Option<Child>) -> bool {
    match child {
        Some(process) => match process.try_wait() {
            Ok(Some(_)) => false,
            Ok(None) => true,
            Err(_) => false,
        },
        None => false,
    }
}

fn port_from_url(url: &str) -> Option<u16> {
    reqwest::Url::parse(url).ok()?.port_or_known_default()
}

fn is_safe_job_id(value: &str) -> bool {
    value.len() == 36 && value.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

fn loopback_base_url(candidate: &str, fallback: &str) -> String {
    let parsed = match reqwest::Url::parse(candidate) {
        Ok(url) => url,
        Err(_) => return fallback.to_string(),
    };
    let host = parsed.host_str().unwrap_or_default();
    let allowed_host = host == "127.0.0.1" || host == "::1";
    if parsed.port_or_known_default() == Some(0) || parsed.scheme() != "http"
        || !allowed_host
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || (parsed.path() != "/" && !parsed.path().is_empty())
    {
        return fallback.to_string();
    }
    parsed.as_str().trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_urls_are_loopback_only() {
        assert_eq!(
            loopback_base_url("http://127.0.0.1:9088", "http://127.0.0.1:8088"),
            "http://127.0.0.1:9088"
        );
        assert_eq!(
            loopback_base_url("http://example.com:8088", "http://127.0.0.1:8088"),
            "http://127.0.0.1:8088"
        );
        assert_eq!(
            loopback_base_url("http://127.0.0.1:0", "http://127.0.0.1:8088"),
            "http://127.0.0.1:8088"
        );
        assert_eq!(
            loopback_base_url("https://127.0.0.1:8088", "http://127.0.0.1:8088"),
            "http://127.0.0.1:8088"
        );
    }

    #[test]
    fn worker_job_ids_must_be_uuid_shaped() {
        assert!(is_safe_job_id("550e8400-e29b-41d4-a716-446655440000"));
        assert!(!is_safe_job_id("../etc/passwd"));
        assert!(!is_safe_job_id("not-a-uuid"));
    }
}

fn inherit_safe_runtime_environment(command: &mut Command) {
    // Preserve only operating-system/runtime essentials. In particular, do
    // not inherit MutexFlow cloud-provider, OAuth, or Supabase credentials.
    for name in [
        "PATH", "HOME", "USERPROFILE", "TMP", "TEMP", "TMPDIR",
        "SYSTEMROOT", "WINDIR", "APPDATA", "LOCALAPPDATA",
        "SSL_CERT_FILE", "SSL_CERT_DIR", "LANG", "LC_ALL", "TZ",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}
