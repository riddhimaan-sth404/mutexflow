use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::routes::auth::AppState;

#[derive(Deserialize)]
pub struct LocalAnalysisRequest {
    pub workspace: String,
}

#[derive(Deserialize)]
pub struct AuditQuery {
    pub limit: Option<usize>,
}

fn error_response(status: StatusCode, message: String) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "error": message })))
}

fn is_uuid(value: &str) -> bool {
    value.len() == 36
        && value.chars().enumerate().all(|(index, ch)| {
            if [8, 13, 18, 23].contains(&index) {
                ch == '-'
            } else {
                ch.is_ascii_hexdigit()
            }
        })
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(health))
        .route("/status", get(status))
        .route("/agents", get(agents))
        .route("/findings", get(findings))
        .route("/approvals", get(pending_approvals))
        .route("/approvals/{id}/approve", post(approve))
        .route("/approvals/{id}/deny", post(deny))
        .route("/audit", get(audit))
        .route("/jobs/local-analysis", post(start_local_analysis))
        .route("/jobs/{id}", get(get_local_analysis_job))
        .route("/jobs/{id}/cancel", post(cancel_local_analysis_job))
}

async fn health(State(state): State<Arc<AppState>>) -> Json<crate::services::orchestral_manager::OrchestralRuntimeStatus> {
    Json(state.orchestral.status().await)
}

async fn status(State(state): State<Arc<AppState>>) -> Json<crate::services::orchestral_manager::OrchestralRuntimeStatus> {
    Json(state.orchestral.status().await)
}

async fn agents(State(state): State<Arc<AppState>>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state.orchestral.core_get("/api/v1/swarm/agents").await
        .map(Json)
        .map_err(|e| error_response(StatusCode::BAD_GATEWAY, e))
}

async fn findings(State(state): State<Arc<AppState>>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state.orchestral.core_get("/api/v1/findings").await
        .map(Json)
        .map_err(|e| error_response(StatusCode::BAD_GATEWAY, e))
}

async fn pending_approvals(State(state): State<Arc<AppState>>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state.orchestral.core_get("/api/v1/hitl/pending").await
        .map(Json)
        .map_err(|e| error_response(StatusCode::BAD_GATEWAY, e))
}

async fn approve(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if !is_uuid(&id) {
        return Err(error_response(StatusCode::BAD_REQUEST, "Invalid approval identifier".to_string()));
    }
    state.orchestral.core_post_empty(&format!("/api/v1/hitl/{id}/approve")).await
        .map(Json)
        .map_err(|e| error_response(StatusCode::BAD_GATEWAY, e))
}

async fn deny(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if !is_uuid(&id) {
        return Err(error_response(StatusCode::BAD_REQUEST, "Invalid approval identifier".to_string()));
    }
    state.orchestral.core_post_empty(&format!("/api/v1/hitl/{id}/deny")).await
        .map(Json)
        .map_err(|e| error_response(StatusCode::BAD_GATEWAY, e))
}

async fn audit(
    State(state): State<Arc<AppState>>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    state.orchestral.core_get(&format!("/api/v1/audit/logs?limit={limit}")).await
        .map(Json)
        .map_err(|e| error_response(StatusCode::BAD_GATEWAY, e))
}

async fn start_local_analysis(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<LocalAnalysisRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    if payload.workspace.trim().is_empty() || payload.workspace.len() > 1024 {
        return Err(error_response(StatusCode::BAD_REQUEST, "workspace must be a non-empty relative path of at most 1024 bytes".to_string()));
    }
    state.orchestral.start_local_analysis(payload.workspace.trim()).await
        .map(|value| (StatusCode::ACCEPTED, Json(value)))
        .map_err(|e| error_response(StatusCode::BAD_GATEWAY, e))
}

async fn cancel_local_analysis_job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if !is_uuid(&id) {
        return Err(error_response(StatusCode::BAD_REQUEST, "Invalid job identifier".to_string()));
    }
    state.orchestral.cancel_local_analysis_job(&id).await
        .map(Json)
        .map_err(|e| error_response(StatusCode::BAD_GATEWAY, e))
}

async fn get_local_analysis_job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if !is_uuid(&id) {
        return Err(error_response(StatusCode::BAD_REQUEST, "Invalid job identifier".to_string()));
    }
    state.orchestral.get_local_analysis_job(&id).await
        .map(Json)
        .map_err(|e| error_response(StatusCode::BAD_GATEWAY, e))
}
