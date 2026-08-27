use crate::models::{ProcessStandardRequest, ProcessStandardResponse};
use crate::routes::auth::AppState;
use crate::services::ai_cascade::execute_cascade;
use crate::services::integration_service::{process_integrations_async, IntegrationPayload};
use axum::{extract::State, http::StatusCode, Json};
use std::sync::Arc;
use tracing::error;

pub async fn process_standard(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ProcessStandardRequest>,
) -> Result<Json<ProcessStandardResponse>, (StatusCode, Json<ProcessStandardResponse>)> {
    if payload.prompt.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ProcessStandardResponse {
                ok: false,
                result: None,
                error: Some("Prompt cannot be empty".to_string()),
            }),
        ));
    }

    if payload.license_key.trim().is_empty() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ProcessStandardResponse {
                ok: false,
                result: None,
                error: Some("License key is required".to_string()),
            }),
        ));
    }

    // Verify license & node-locking
    let mut resolved_machine_id = payload.machine_id.clone();
    if let Some(supabase) = &state.supabase {
        match supabase.get_license(&payload.license_key).await {
            Ok(Some(rec)) => {
                if let Some(ref bound) = rec.machine_id {
                    if let Some(ref req_mach) = payload.machine_id {
                        if bound != req_mach {
                            return Err((
                                StatusCode::FORBIDDEN,
                                Json(ProcessStandardResponse {
                                    ok: false,
                                    result: None,
                                    error: Some("License is locked to another machine".to_string()),
                                }),
                            ));
                        }
                    }
                    resolved_machine_id = Some(bound.clone());
                }
            }
            Ok(None) => {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(ProcessStandardResponse {
                        ok: false,
                        result: None,
                        error: Some("Invalid or expired license".to_string()),
                    }),
                ));
            }
            Err(e) => {
                error!("[Agent] Failed to check license: {}", e);
            }
        }
    }

    // Execute Cloud Cascade
    let result = match execute_cascade(
        &payload.prompt,
        payload.workflow_type.as_deref(),
        payload.task_complexity.as_deref(),
    )
    .await
    {
        Ok(res) => res,
        Err(err) => {
            return Err((
                StatusCode::BAD_GATEWAY,
                Json(ProcessStandardResponse {
                    ok: false,
                    result: None,
                    error: Some(err),
                }),
            ));
        }
    };

    // Asynchronously dispatch integrations if specified
    if let Some(targets) = payload.target_integrations {
        if !targets.is_empty() {
            let supabase_clone = state.supabase.clone();
            let integration_payload = IntegrationPayload {
                result: result.clone(),
                workflow_type: payload.workflow_type.clone(),
                task_complexity: payload.task_complexity.clone(),
                prompt: Some(payload.prompt.clone()),
            };

            tokio::spawn(async move {
                process_integrations_async(supabase_clone, resolved_machine_id, targets, integration_payload).await;
            });
        }
    }

    Ok(Json(ProcessStandardResponse {
        ok: true,
        result: Some(result),
        error: None,
    }))
}
