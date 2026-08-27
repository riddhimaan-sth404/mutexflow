use crate::config::db::SupabaseClient;
use reqwest::Client;
use serde_json::Value;
use std::env;
use tracing::{error, info, warn};

pub struct IntegrationPayload {
    pub result: String,
    pub workflow_type: Option<String>,
    pub task_complexity: Option<String>,
    pub prompt: Option<String>,
}

pub async fn refresh_google_token(
    supabase: &SupabaseClient,
    machine_id: &str,
    refresh_token: &str,
) -> Option<String> {
    let client_id = env::var("GOOGLE_CLIENT_ID").ok()?;
    let client_secret = env::var("GOOGLE_CLIENT_SECRET").ok()?;

    let client = Client::new();
    let params = [
        ("client_id", client_id.as_str()),
        ("client_secret", client_secret.as_str()),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ];

    let resp = client
        .post("https://oauth2.googleapis.com/token")
        .form(&params)
        .send()
        .await
        .ok()?;

    let json: Value = resp.json().await.ok()?;
    let new_access_token = json["access_token"].as_str()?.to_string();

    let _ = supabase
        .upsert_integration(machine_id, "google", &new_access_token, Some(refresh_token))
        .await;

    Some(new_access_token)
}

pub async fn send_slack(token: &str, payload: &IntegrationPayload) -> Result<(), String> {
    let channel = env::var("SLACK_CHANNEL").unwrap_or_else(|_| "#general".to_string());
    let client = Client::new();
    let text = format!(
        "*MutexFlow Agent Output*\n*Workflow:* {}\n*Complexity:* {}\n*Result:*\n```\n{}\n```",
        payload.workflow_type.as_deref().unwrap_or("general"),
        payload.task_complexity.as_deref().unwrap_or("medium"),
        payload.result
    );

    let body = serde_json::json!({
        "channel": channel,
        "text": text
    });

    let resp = client
        .post("https://slack.com/api/chat.postMessage")
        .header("Authorization", format!("Bearer {}", token))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let json: Value = resp.json().await.map_err(|e| e.to_string())?;
    if json["ok"].as_bool().unwrap_or(false) {
        info!("[Integration:Slack] Successfully posted to {}", channel);
        Ok(())
    } else {
        let err = json["error"].as_str().unwrap_or("unknown error");
        Err(format!("Slack API error: {}", err))
    }
}

pub async fn append_google_sheet(token: &str, payload: &IntegrationPayload) -> Result<(), String> {
    let sheet_id = env::var("GOOGLE_SHEET_ID").map_err(|_| "GOOGLE_SHEET_ID not set".to_string())?;
    let client = Client::new();
    let url = format!(
        "https://sheets.googleapis.com/v4/spreadsheets/{}/values/A1:append?valueInputOption=USER_ENTERED",
        urlencoding::encode(&sheet_id)
    );

    let now = chrono::Utc::now().to_rfc3339();
    let row = serde_json::json!({
        "values": [[
            payload.workflow_type.as_deref().unwrap_or("general"),
            payload.task_complexity.as_deref().unwrap_or("medium"),
            payload.prompt.as_deref().unwrap_or(""),
            payload.result,
            now
        ]]
    });

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .json(&row)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if resp.status().is_success() {
        info!("[Integration:GoogleSheets] Row appended successfully");
        Ok(())
    } else {
        let err = resp.text().await.unwrap_or_default();
        Err(format!("Google Sheets error: {}", err))
    }
}

pub async fn create_github_issue(token: &str, payload: &IntegrationPayload) -> Result<(), String> {
    let target_repo = env::var("GITHUB_TARGET_REPO").unwrap_or_else(|_| "mutexflow/agent-results".to_string());
    let client = Client::new();
    let url = format!("https://api.github.com/repos/{}/issues", target_repo);

    let title = format!(
        "Agent Result - {} - {}",
        payload.workflow_type.as_deref().unwrap_or("general"),
        chrono::Utc::now().format("%Y-%m-%d")
    );

    let body = format!(
        "## MutexFlow Agent Result\n\n**Workflow:** {}\n**Complexity:** {}\n**Prompt:** {}\n\n### Result\n```\n{}\n```",
        payload.workflow_type.as_deref().unwrap_or("N/A"),
        payload.task_complexity.as_deref().unwrap_or("N/A"),
        payload.prompt.as_deref().unwrap_or("N/A"),
        payload.result
    );

    let payload_json = serde_json::json!({
        "title": title,
        "body": body,
        "labels": ["ai-agent", "automated"]
    });

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "MutexFlow-Rust-Backend")
        .header("Accept", "application/vnd.github+json")
        .json(&payload_json)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if resp.status().is_success() {
        info!("[Integration:GitHub] Issue created successfully in {}", target_repo);
        Ok(())
    } else {
        let err = resp.text().await.unwrap_or_default();
        Err(format!("GitHub API error: {}", err))
    }
}

pub async fn process_integrations_async(
    supabase: Option<SupabaseClient>,
    machine_id: Option<String>,
    target_integrations: Vec<String>,
    payload: IntegrationPayload,
) {
    let machine_id = match machine_id {
        Some(m) if !m.is_empty() => m,
        _ => {
            info!("[Integrations] No machine ID provided — skipping integrations dispatch");
            return;
        }
    };

    let supabase = match supabase {
        Some(s) => s,
        None => {
            warn!("[Integrations] Supabase is not configured — unable to fetch OAuth tokens");
            return;
        }
    };

    for target in target_integrations {
        let provider = match target.as_str() {
            "google_sheets" | "gmail" => "google",
            other => other,
        };

        if let Ok(Some(integration)) = supabase.get_integration(&machine_id, provider).await {
            let mut access_token = integration.access_token;

            // Attempt token refresh for google if needed
            if provider == "google" {
                if let Some(refresh_token) = integration.refresh_token.as_deref() {
                    if let Some(new_tok) = refresh_google_token(&supabase, &machine_id, refresh_token).await {
                        access_token = new_tok;
                    }
                }
            }

            match target.as_str() {
                "slack" => {
                    if let Err(e) = send_slack(&access_token, &payload).await {
                        error!("[Integrations:Slack] Error: {}", e);
                    }
                }
                "google_sheets" | "google" => {
                    if let Err(e) = append_google_sheet(&access_token, &payload).await {
                        error!("[Integrations:GoogleSheets] Error: {}", e);
                    }
                }
                "github" => {
                    if let Err(e) = create_github_issue(&access_token, &payload).await {
                        error!("[Integrations:GitHub] Error: {}", e);
                    }
                }
                other => {
                    info!("[Integrations:{}] Integration dispatch acknowledged (handler in progress)", other);
                }
            }
        } else {
            info!("[Integrations] No connected token found for provider '{}' on machine '{}'", provider, machine_id);
        }
    }
}
