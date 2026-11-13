use axum::{
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use rand::RngExt;
use serde_json::json;
use sha2::{Digest, Sha256};

pub fn generate_api_key() -> (String, String, String) {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    let hex_part = hex::encode(bytes);
    let full_key = format!("orch_live_{}", hex_part);
    let prefix = format!("orch_live_{}...", &hex_part[..6]);
    let hash = hash_api_key(&full_key);
    (full_key, prefix, hash)
}

pub fn hash_api_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

#[derive(Debug)]
pub enum AuthError {
    MissingHeader,
    InvalidKey,
    #[allow(dead_code)]
    InsufficientPermissions(String),
    NotFound,
    Internal(String),
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, err_msg) = match self {
            AuthError::MissingHeader => (
                StatusCode::UNAUTHORIZED,
                "Missing 'X-Orchestral-API-Key' or 'Authorization: Bearer <key>' header".to_string(),
            ),
            AuthError::InvalidKey => (
                StatusCode::UNAUTHORIZED,
                "Invalid or revoked API key".to_string(),
            ),
            AuthError::InsufficientPermissions(scope) => (
                StatusCode::FORBIDDEN,
                format!("Insufficient permissions: {}", scope),
            ),
            AuthError::NotFound => (
                StatusCode::NOT_FOUND,
                "Resource not found".to_string(),
            ),
            AuthError::Internal(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Internal server error: {}", msg),
            ),
        };

        (
            status,
            Json(json!({
                "error": err_msg,
                "status": status.as_u16()
            })),
        )
            .into_response()
    }
}

pub fn extract_raw_key_from_parts(parts: &Parts) -> Option<String> {
    if let Some(h) = parts.headers.get("X-Orchestral-API-Key") {
        if let Ok(val) = h.to_str() {
            return Some(val.trim().to_string());
        }
    }
    if let Some(h) = parts.headers.get("Authorization") {
        if let Ok(val) = h.to_str() {
            let val = val.trim();
            if val.starts_with("Bearer ") {
                return Some(val[7..].trim().to_string());
            }
        }
    }
    None
}
