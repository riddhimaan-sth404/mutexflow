use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use crate::routes::auth::AppState;

#[derive(Deserialize)]
struct CreateSessionRequest { workspace: String }

#[derive(Deserialize)]
struct RunRequest { prompt: String }

fn failure(message: String) -> (StatusCode, Json<Value>) {
    (StatusCode::BAD_GATEWAY, Json(json!({ "error": message })))
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(health))
        .route("/sessions", get(sessions).post(create_session))
        .route("/sessions/:id/run", post(run))
        .route("/sessions/:id/cancel", post(cancel))
}

async fn health(State(state): State<Arc<AppState>>) -> Json<crate::services::jcode_manager::JcodeStatus> {
    Json(state.jcode.status())
}

async fn sessions(State(state): State<Arc<AppState>>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state.jcode.sessions().await.map(Json).map_err(failure)
}

async fn create_session(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    state.jcode.create_session(&request.workspace).await
        .map(|value| (StatusCode::CREATED, Json(value)))
        .map_err(failure)
}

async fn run(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(request): Json<RunRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state.jcode.run(&id, &request.prompt).await.map(Json).map_err(failure)
}

async fn cancel(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state.jcode.cancel(&id).await.map(Json).map_err(failure)
}
