use axum::{
    routing::{get, post},
    Router,
};
use backend_rust::config::db::SupabaseClient;
use backend_rust::routes;
use backend_rust::routes::auth::AppState;
use backend_rust::services::orchestral_manager::OrchestralManager;
use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "backend_rust=info,tower_http=info".into()),
        )
        .init();

    let port: u16 = env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3001);

    let supabase = SupabaseClient::from_env();
    if supabase.is_some() {
        info!("[Supabase] Connected to database backend");
    } else {
        info!("[Supabase] Running in standalone / developer mode (no Supabase credentials set)");
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();
    let orchestral = Arc::new(OrchestralManager::new(root));
    if let Err(error) = orchestral.start().await {
        tracing::error!("[Orchestral] Runtime startup failed: {}", error);
    }

    let state = Arc::new(AppState { supabase, port, orchestral: orchestral.clone() });

    // The desktop talks to the backend directly (without browser CORS). For
    // browser-based local tooling, permit loopback origins only.
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _| {
            origin.to_str().map(|value| {
                value == "http://localhost"
                    || value == "https://localhost"
                    || value.starts_with("http://localhost:")
                    || value.starts_with("https://localhost:")
                    || value == "http://127.0.0.1"
                    || value == "https://127.0.0.1"
                    || value.starts_with("http://127.0.0.1:")
                    || value.starts_with("https://127.0.0.1:")
                    || value.starts_with("http://[::1]:")
                    || value.starts_with("https://[::1]:")
            }).unwrap_or(false)
        }))
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/health", get(|| async { axum::Json(serde_json::json!({ "status": "ok", "service": "mutexflow-backend" })) }))
        .route("/api/v1/auth/verify-license", post(routes::auth::verify_license))
        .route("/api/v1/auth/:provider/connect", get(routes::auth::oauth_connect))
        .route("/api/v1/auth/:provider/callback", get(routes::auth::oauth_callback))
        .route("/api/v1/agent/process-standard", post(routes::agent::process_standard))
        .route("/api/v1/integrations/dispatch", post(routes::integrations::dispatch_integrations))
        .route("/api/v1/integrations/status", get(routes::integrations::get_integrations_status))
        .nest("/api/v1/orchestral", routes::orchestral::router())
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let bind_host = env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1".to_string());
    let addr: SocketAddr = format!("{bind_host}:{port}")
        .parse()
        .unwrap_or_else(|_| SocketAddr::from(([127, 0, 0, 1], port)));
    info!("[MutexFlow Rust] Server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let server_result = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            info!("Shutdown signal received");
        })
        .await;

    orchestral.shutdown().await;
    server_result?;
    Ok(())
}
