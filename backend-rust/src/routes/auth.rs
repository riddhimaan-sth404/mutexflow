use crate::config::db::SupabaseClient;
use crate::config::oauth::get_oauth_config;
use crate::models::{VerifyLicenseRequest, VerifyLicenseResponse};
use axum::{
    extract::{Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{Html, IntoResponse, Redirect, Response},
    Json,
};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use std::env;
use std::fs;
use std::sync::{Arc, OnceLock};
use tracing::{error, info, warn};

#[derive(Clone)]
pub struct AppState {
    pub supabase: Option<SupabaseClient>,
    pub port: u16,
    pub orchestral: Arc<crate::services::orchestral_manager::OrchestralManager>,
    pub jcode: Arc<crate::services::jcode_manager::JcodeManager>,
}

#[derive(Debug, Deserialize)]
pub struct ConnectQuery {
    #[serde(rename = "machineId")]
    pub machine_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
}

pub async fn verify_license(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<VerifyLicenseRequest>,
) -> Result<Json<VerifyLicenseResponse>, (StatusCode, Json<VerifyLicenseResponse>)> {
    if payload.license_key.trim().is_empty() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(VerifyLicenseResponse {
                ok: false,
                error: Some("License key cannot be empty".to_string()),
            }),
        ));
    }

    if payload.machine_id.trim().is_empty() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(VerifyLicenseResponse {
                ok: false,
                error: Some("Machine ID is required for hardware node-locking".to_string()),
            }),
        ));
    }

    let supabase = match &state.supabase {
        Some(s) => s,
        None => {
            warn!("[Auth] Supabase is not configured — allowing license in developer mode");
            return Ok(Json(VerifyLicenseResponse { ok: true, error: None }));
        }
    };

    match supabase.get_license(&payload.license_key).await {
        Ok(Some(record)) => {
            match record.machine_id {
                None => {
                    // License is unbound, bind to this machine
                    if let Some(id) = &record.id {
                        let _ = supabase.bind_license_machine(id, &payload.machine_id).await;
                        info!("[Auth] License bound to machine ID: {}", payload.machine_id);
                    }
                    Ok(Json(VerifyLicenseResponse { ok: true, error: None }))
                }
                Some(ref bound_machine) if bound_machine == &payload.machine_id => {
                    Ok(Json(VerifyLicenseResponse { ok: true, error: None }))
                }
                Some(_) => {
                    Err((
                        StatusCode::FORBIDDEN,
                        Json(VerifyLicenseResponse {
                            ok: false,
                            error: Some("This license is already bound to another machine.".to_string()),
                        }),
                    ))
                }
            }
        }
        Ok(None) => Err((
            StatusCode::FORBIDDEN,
            Json(VerifyLicenseResponse {
                ok: false,
                error: Some("Invalid or inactive license key".to_string()),
            }),
        )),
        Err(e) => {
            error!("[Auth] Database query error: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(VerifyLicenseResponse {
                    ok: false,
                    error: Some("Database verification error".to_string()),
                }),
            ))
        }
    }
}

pub async fn oauth_connect(
    State(state): State<Arc<AppState>>,
    Path(provider): Path<String>,
    Query(query): Query<ConnectQuery>,
) -> Response {
    let machine_id = match query.machine_id {
        Some(id) if !id.trim().is_empty() => id,
        _ => return (StatusCode::BAD_REQUEST, "Missing machine ID").into_response(),
    };

    let config = match get_oauth_config(&provider) {
        Some(c) => c,
        None => return (StatusCode::BAD_REQUEST, format!("Unknown provider: {}", provider)).into_response(),
    };

    let client_id = match env::var(config.client_id_env) {
        Ok(val) if !val.trim().is_empty() => val,
        _ => {
            return (
                StatusCode::NOT_IMPLEMENTED,
                format!("Missing {} in server environment variables", config.client_id_env),
            )
                .into_response();
        }
    };

    let backend_url = env::var("BACKEND_URL").unwrap_or_else(|_| format!("http://localhost:{}", state.port));
    let redirect_uri = format!("{}/api/v1/auth/{}/callback", backend_url, provider);

    let mut auth_url = format!(
        "{}?client_id={}&scope={}&redirect_uri={}&state={}",
        config.authorize_url,
        urlencoding::encode(&client_id),
        urlencoding::encode(config.scope),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(&machine_id),
    );

    if let Some(extra) = config.extra_params {
        auth_url.push_str(extra);
    }

    Redirect::temporary(&auth_url).into_response()
}

pub async fn oauth_callback(
    State(state): State<Arc<AppState>>,
    Path(provider): Path<String>,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let code = match query.code {
        Some(c) if !c.trim().is_empty() => c,
        _ => return (StatusCode::BAD_REQUEST, "Missing authorization code").into_response(),
    };

    let machine_id = match query.state {
        Some(s) if !s.trim().is_empty() => s,
        _ => return (StatusCode::BAD_REQUEST, "Missing state (machine ID)").into_response(),
    };

    let config = match get_oauth_config(&provider) {
        Some(c) => c,
        None => return (StatusCode::BAD_REQUEST, format!("Unknown provider: {}", provider)).into_response(),
    };

    let client_id = env::var(config.client_id_env).unwrap_or_default();
    let client_secret = env::var(config.client_secret_env).unwrap_or_default();

    if client_id.is_empty() || client_secret.is_empty() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "Server OAuth credentials not configured").into_response();
    }

    let backend_url = env::var("BACKEND_URL").unwrap_or_else(|_| format!("http://localhost:{}", state.port));
    let redirect_uri = format!("{}/api/v1/auth/{}/callback", backend_url, provider);

    let client = Client::new();
    let token_resp = if provider == "github" {
        let body = serde_json::json!({
            "client_id": client_id,
            "client_secret": client_secret,
            "code": code,
            "redirect_uri": redirect_uri
        });

        client
            .post(config.token_url)
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .await
    } else {
        let params = [
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("code", code.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri.as_str()),
        ];

        client
            .post(config.token_url)
            .header("Accept", "application/json")
            .form(&params)
            .send()
            .await
    };

    match token_resp {
        Ok(resp) => {
            let json: Value = match resp.json().await {
                Ok(j) => j,
                Err(e) => {
                    return (StatusCode::BAD_GATEWAY, format!("Token exchange parse error: {}", e)).into_response()
                }
            };

            let access_token = match json["access_token"].as_str() {
                Some(tok) => tok,
                None => {
                    return (StatusCode::BAD_GATEWAY, format!("No access token returned: {:?}", json)).into_response()
                }
            };

            let refresh_token = json["refresh_token"].as_str();

            if let Some(supabase) = &state.supabase {
                let _ = supabase
                    .upsert_integration(&machine_id, &provider, access_token, refresh_token)
                    .await;
            }

            // Write signal file for backward compatibility
            let mut temp_dir = std::env::temp_dir();
            temp_dir.push("mutexflow-oauth");
            let _ = fs::create_dir_all(&temp_dir);
            let signal_file = temp_dir.join(format!("{}_{}", machine_id, provider));
            let _ = fs::write(signal_file, chrono::Utc::now().timestamp_millis().to_string());

            let html = format!(
                r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>MutexFlow - {} Connected</title>
  <style>
    body {{
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
      background: #121420;
      color: #e8e6f0;
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      height: 100vh;
      margin: 0;
    }}
    .card {{
      background: #161824;
      border: 1px solid #2a2d3e;
      border-radius: 12px;
      padding: 2.5rem;
      text-align: center;
      max-width: 420px;
    }}
    h1 {{ color: #6ee7b7; font-size: 1.5rem; margin-bottom: 0.5rem; }}
    p {{ color: #9d9bb0; font-size: 0.95rem; line-height: 1.5; }}
  </style>
</head>
<body>
  <div class="card">
    <h1>✅ {} Connected</h1>
    <p>Authentication succeeded. You can now close this tab and return to MutexFlow.</p>
  </div>
</body>
</html>"#,
                provider, provider
            );

            Html(html).into_response()
        }
        Err(e) => (StatusCode::BAD_GATEWAY, format!("Token exchange failed: {}", e)).into_response(),
    }
}

pub const LOCAL_TOKEN_HEADER: &str = "x-mutexflow-token";

static LOCAL_API_TOKEN: OnceLock<String> = OnceLock::new();

/// Resolve or create the loopback API token that protects the local
/// orchestral and JCode endpoints. The token comes from MUTEXFLOW_API_TOKEN
/// when set, otherwise it is generated once and persisted owner-only at
/// `.mutexflow/backend-token`.
pub fn ensure_local_api_token(root: &std::path::Path) -> String {
    LOCAL_API_TOKEN.get().cloned().unwrap_or_else(|| {
        let token = configured_local_token().unwrap_or_else(|| load_or_create_token_file(root));
        let _ = LOCAL_API_TOKEN.set(token.clone());
        token
    })
}

fn configured_local_token() -> Option<String> {
    configured_local_token_value(std::env::var("MUTEXFLOW_API_TOKEN").ok().as_deref())
}

fn configured_local_token_value(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|value| value.len() >= 16).map(str::to_string)
}

fn load_or_create_token_file(root: &std::path::Path) -> String {
    use std::io::Write as _;

    let dir = root.join(".mutexflow");
    let path = dir.join("backend-token");
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|error| panic!("create private MutexFlow state directory: {error}"));
    let directory = std::fs::symlink_metadata(&dir)
        .unwrap_or_else(|error| panic!("inspect MutexFlow state directory: {error}"));
    if directory.file_type().is_symlink() || !directory.is_dir() {
        panic!("MutexFlow state directory must be a real directory, not a symlink");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if directory.uid() != unsafe { libc::geteuid() } {
            panic!("MutexFlow state directory must be owned by the current user");
        }
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
            .unwrap_or_else(|error| panic!("restrict MutexFlow state directory: {error}"));
    }

    loop {
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    panic!("MutexFlow backend token must be a regular file, not a symlink");
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::{MetadataExt, PermissionsExt};
                    if metadata.uid() != unsafe { libc::geteuid() } {
                        panic!("MutexFlow backend token must be owned by the current user");
                    }
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                        .unwrap_or_else(|error| panic!("restrict MutexFlow backend token: {error}"));
                }
                let token = std::fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("read MutexFlow backend token: {error}"));
                let token = token.trim().to_string();
                if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    panic!("MutexFlow backend-token file is invalid; remove it and restart to generate a new token");
                }
                return token;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let token = generate_local_token();
                let mut options = std::fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                match options.open(&path) {
                    Ok(mut file) => {
                        file.write_all(token.as_bytes())
                            .and_then(|_| file.write_all(b"\n"))
                            .and_then(|_| file.sync_all())
                            .unwrap_or_else(|error| panic!("persist MutexFlow backend token: {error}"));
                        return token;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("create MutexFlow backend token: {error}"),
                }
            }
            Err(error) => panic!("inspect MutexFlow backend token: {error}"),
        }
    }
}

/// Generate a random local token (hex-encoded 256 bits) using the operating
/// system cryptographic random-number generator. Fail closed if it is unavailable.
pub fn generate_local_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes)
        .expect("OS cryptographic random source unavailable; refusing to create a local API token");
    hex_encode(&bytes)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Axum middleware protecting the local orchestral and JCode routers: the
/// request must arrive on a loopback Host header and carry the shared local
/// token. Unauthenticated callers receive 401.
pub async fn require_local_access(
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    let host = headers.get(axum::http::header::HOST).and_then(|value| value.to_str().ok());
    let token = headers.get(LOCAL_TOKEN_HEADER).and_then(|value| value.to_str().ok());
    let expected = expected_local_token();
    if !authorize_request(host, token, &expected) {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "unauthorized" }))).into_response();
    }
    next.run(request).await
}

fn expected_local_token() -> String {
    configured_local_token().or_else(|| LOCAL_API_TOKEN.get().cloned()).unwrap_or_default()
}

/// Pure authorization decision: loopback Host (DNS-rebinding defense) plus an
/// exact, constant-time token match. Fails closed when no token is configured.
fn authorize_request(host: Option<&str>, provided_token: Option<&str>, expected_token: &str) -> bool {
    let Some(host) = host else { return false; };
    let host_name = if host.starts_with('[') {
        host.split(']').next().unwrap_or("").trim_start_matches('[')
    } else {
        host.rsplit_once(':').map(|(name, _)| name).unwrap_or(host)
    };
    if !matches!(host_name, "127.0.0.1" | "::1" | "localhost") {
        return false;
    }
    if expected_token.is_empty() {
        return false;
    }
    match provided_token {
        Some(token) => constant_time_eq(token.as_bytes(), expected_token.as_bytes()),
        None => false,
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).fold(0u8, |accumulator, (x, y)| accumulator | (x ^ y)) == 0
}

#[cfg(test)]
mod local_access_tests {
    use super::*;

    #[test]
    fn unauthenticated_callers_are_rejected() {
        let token = "token-abcdef0123456789";
        assert!(!authorize_request(Some("127.0.0.1:3001"), None, token));
        assert!(!authorize_request(Some("127.0.0.1:3001"), Some("wrong-token"), token));
        assert!(!authorize_request(Some("127.0.0.1:3001"), Some(""), token));
        assert!(!authorize_request(None, Some(token), token));
        // No token configured: fail closed even for loopback callers.
        assert!(!authorize_request(Some("127.0.0.1:3001"), Some(token), ""));
    }

    #[test]
    fn non_loopback_hosts_are_rejected_even_with_a_valid_token() {
        let token = "token-abcdef0123456789";
        assert!(!authorize_request(Some("evil.example.com:3001"), Some(token), token));
        assert!(!authorize_request(Some("192.168.1.10:3001"), Some(token), token));
        assert!(!authorize_request(Some("[fe80::1]:3001"), Some(token), token));
    }

    #[test]
    fn loopback_callers_with_the_expected_token_are_accepted() {
        let token = "token-abcdef0123456789";
        assert!(authorize_request(Some("127.0.0.1:3001"), Some(token), token));
        assert!(authorize_request(Some("localhost:3001"), Some(token), token));
        assert!(authorize_request(Some("[::1]:3001"), Some(token), token));
    }

    #[test]
    fn token_comparison_is_exact() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }

    #[test]
    fn generated_tokens_are_long_and_hex() {
        let token = generate_local_token();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|character| character.is_ascii_hexdigit()));
        assert_ne!(generate_local_token(), generate_local_token());
    }

    #[test]
    fn short_environment_tokens_are_ignored_consistently() {
        assert!(configured_local_token_value(Some("short")).is_none());
        assert_eq!(
            configured_local_token_value(Some("0123456789abcdef")),
            Some("0123456789abcdef".to_string())
        );
    }
}
