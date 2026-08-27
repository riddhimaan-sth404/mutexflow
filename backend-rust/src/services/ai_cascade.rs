use crate::config::ai_providers::CLOUD_CASCADES;
use crate::services::agent_prompts::get_prompt;
use reqwest::Client;
use serde_json::Value;
use std::env;
use std::time::Duration;
use tracing::{info, warn};

pub async fn execute_cascade(
    prompt: &str,
    workflow_type: Option<&str>,
    _complexity: Option<&str>,
) -> Result<String, String> {
    let agent_prompt = get_prompt(workflow_type);
    let client = Client::builder()
        .timeout(Duration::from_secs(25))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let messages = serde_json::json!([
        { "role": "system", "content": agent_prompt.system },
        { "role": "user", "content": prompt }
    ]);

    for provider in CLOUD_CASCADES {
        let api_key = match env::var(provider.env_key) {
            Ok(key) if !key.trim().is_empty() => key,
            _ => {
                // Skip provider if no key configured
                continue;
            }
        };

        info!("[Cascade] Trying provider: {}", provider.name);

        let body = serde_json::json!({
            "model": provider.model,
            "messages": messages
        });

        let resp = client
            .post(provider.url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&body)
            .send()
            .await;

        match resp {
            Ok(response) => {
                if response.status().is_success() {
                    match response.json::<Value>().await {
                        Ok(json) => {
                            if let Some(content) = json["choices"][0]["message"]["content"].as_str() {
                                info!("[Cascade] {} succeeded!", provider.name);
                                return Ok(content.to_string());
                            } else if let Some(content) = json["choices"][0]["text"].as_str() {
                                info!("[Cascade] {} succeeded (text field)!", provider.name);
                                return Ok(content.to_string());
                            } else {
                                warn!("[Cascade] {} returned unexpected JSON format", provider.name);
                            }
                        }
                        Err(e) => {
                            warn!("[Cascade] Failed to parse JSON from {}: {}", provider.name, e);
                        }
                    }
                } else {
                    let status = response.status();
                    let err_text = response.text().await.unwrap_or_default();
                    warn!("[Cascade] {} returned status {} - {}", provider.name, status, err_text);
                }
            }
            Err(e) => {
                warn!("[Cascade] Request to {} failed: {}", provider.name, e);
            }
        }
    }

    Err("None of the cloud AI providers could process the request. Please check API keys or network connection.".to_string())
}
