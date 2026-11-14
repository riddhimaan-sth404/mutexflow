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
struct CreateSessionRequest { workspace: String, role: Option<String>, objective: Option<String> }

#[derive(Deserialize)]
struct RunRequest { prompt: String }

#[derive(Deserialize)]
struct BatchRequest { workspace: String, tasks: Vec<String> }

#[derive(Deserialize)]
struct EventsQuery { after: Option<u64> }

fn failure(message: String) -> (StatusCode, Json<Value>) {
    (StatusCode::BAD_GATEWAY, Json(json!({ "error": message })))
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(health))
        .route("/sessions", get(sessions).post(create_session))
        .route("/events", get(events))
        .route("/swarm", post(run_batch))
        .route("/swarm/:id", get(batch_status))
        .route("/swarm/:id/cancel", post(cancel_batch))
        .route("/sessions/:id/run", post(run))
        .route("/sessions/:id/cancel", post(cancel))
}

async fn health(State(state): State<Arc<AppState>>) -> Json<crate::services::jcode_manager::JcodeStatus> {
    Json(state.jcode.status())
}

async fn events(
    State(state): State<Arc<AppState>>,
    Query(query): Query<EventsQuery>,
) -> Json<Value> {
    let after = query.after.unwrap_or(0);
    Json(json!({ "events": state.jcode.events_since(after), "next_sequence": state.jcode.latest_event_sequence() }))
}

async fn sessions(State(state): State<Arc<AppState>>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state.jcode.sessions().await.map(Json).map_err(failure)
}

async fn create_session(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    state.jcode.create_agent_session(
        &request.workspace,
        request.role.as_deref().unwrap_or("workspace_analyst"),
        request.objective.as_deref().unwrap_or("Analyze this workspace and report evidence-backed observations."),
    ).await
        .map(|value| (StatusCode::CREATED, Json(value)))
        .map_err(failure)
}


async fn run_batch(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BatchRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    state.jcode.start_batch(&request.workspace, request.tasks).await
        .map(|value| (StatusCode::ACCEPTED, Json(value)))
        .map_err(failure)
}

async fn batch_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state.jcode.batch_status(&id).map(Json).map_err(failure)
}

async fn cancel_batch(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state.jcode.cancel_batch(&id).await.map(Json).map_err(failure)
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
