use crate::config::db::SupabaseClient;
use crate::config::oauth::get_oauth_config;
use crate::models::{VerifyLicenseRequest, VerifyLicenseResponse};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    Json,
};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use std::env;
use std::fs;
use std::sync::Arc;
use tracing::{error, info, warn};

#[derive(Clone)]
pub struct AppState {
    pub supabase: Option<SupabaseClient>,
    pub port: u16,
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
