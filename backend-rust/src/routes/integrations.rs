use crate::models::{
    DispatchIntegrationsRequest, DispatchIntegrationsResponse, IntegrationStatusItem, IntegrationsStatusResponse,
};
use crate::routes::auth::AppState;
use crate::services::integration_service::{process_integrations_async, IntegrationPayload};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct StatusQuery {
    #[serde(rename = "machineId")]
    pub machine_id: Option<String>,
}

pub async fn dispatch_integrations(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<DispatchIntegrationsRequest>,
) -> Result<Json<DispatchIntegrationsResponse>, (StatusCode, Json<DispatchIntegrationsResponse>)> {
    if payload.result.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(DispatchIntegrationsResponse {
                ok: false,
                error: Some("Result cannot be empty".to_string()),
            }),
        ));
    }

    if let Some(targets) = payload.target_integrations {
        if !targets.is_empty() {
            let supabase_clone = state.supabase.clone();
            let integration_payload = IntegrationPayload {
                result: payload.result,
                workflow_type: payload.workflow_type,
                task_complexity: payload.task_complexity,
                prompt: payload.prompt,
            };

            tokio::spawn(async move {
                process_integrations_async(supabase_clone, payload.machine_id, targets, integration_payload).await;
            });
        }
    }

    Ok(Json(DispatchIntegrationsResponse { ok: true, error: None }))
}

pub async fn get_integrations_status(
    State(state): State<Arc<AppState>>,
    Query(query): Query<StatusQuery>,
) -> Json<IntegrationsStatusResponse> {
    let known_providers = [
        "slack", "gmail", "google_sheets", "notion", "confluence", "github", "jira", "linear", "salesforce", "hubspot",
    ];

    let machine_id = match query.machine_id {
        Some(m) if !m.is_empty() => m,
        _ => {
            return Json(IntegrationsStatusResponse {
                ok: true,
                integrations: known_providers
                    .iter()
                    .map(|p| IntegrationStatusItem {
                        provider: p.to_string(),
                        connected: false,
                        updated_at: None,
                    })
                    .collect(),
            });
        }
    };

    let mut connected_map = std::collections::HashMap::new();
    if let Some(supabase) = &state.supabase {
        if let Ok(records) = supabase.get_all_integrations(&machine_id).await {
            for rec in records {
                connected_map.insert(rec.provider, rec.updated_at);
            }
        }
    }

    let items = known_providers
        .iter()
        .map(|p| {
            let actual_provider = match *p {
                "google_sheets" | "gmail" => "google",
                other => other,
            };
            let is_connected = connected_map.contains_key(actual_provider);
            let updated = connected_map.get(actual_provider).cloned().flatten();
            IntegrationStatusItem {
                provider: p.to_string(),
                connected: is_connected,
                updated_at: updated,
            }
        })
        .collect();

    Json(IntegrationsStatusResponse {
        ok: true,
        integrations: items,
    })
}
