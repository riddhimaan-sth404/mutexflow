use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Serialize)]
pub struct OrchestralHealth {
    pub connected: bool,
    pub core_url: String,
    pub http_status: Option<u16>,
    pub core_response: Option<Value>,
    pub error: Option<String>,
}

pub async fn check_health() -> OrchestralHealth {
    let core_url = std::env::var("ORCHESTRAL_CORE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8088".to_string())
        .trim_end_matches('/')
        .to_string();

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            tracing::error!("[Orchestral] Failed to initialize HTTP client: {error}");
            return OrchestralHealth {
                connected: false,
                core_url,
                http_status: None,
                core_response: None,
                error: Some("Unable to initialize Orchestral health client".to_string()),
            };
        }
    };

    match client.get(format!("{core_url}/health")).send().await {
        Ok(response) => {
            let status = response.status();
            let status_code = status.as_u16();
            let body = response.json::<Value>().await.ok();
            OrchestralHealth {
                connected: status.is_success(),
                core_url,
                http_status: Some(status_code),
                core_response: body,
                error: if status.is_success() {
                    None
                } else {
                    Some("Orchestral Citadel returned an unsuccessful status".to_string())
                },
            }
        }
        Err(error) => {
            tracing::warn!("[Orchestral] Health check failed: {error}");
            OrchestralHealth {
                connected: false,
                core_url,
                http_status: None,
                core_response: None,
                error: Some("Orchestral Citadel is unreachable".to_string()),
            }
        }
    }
}
