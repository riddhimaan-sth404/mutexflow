use axum::Json;

use crate::services::orchestral::{self, OrchestralHealth};

pub async fn health() -> Json<OrchestralHealth> {
    Json(orchestral::check_health().await)
}
