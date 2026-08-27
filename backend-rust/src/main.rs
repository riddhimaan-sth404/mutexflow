use axum::{
    routing::{get, post},
    Router,
};
use backend_rust::config::db::SupabaseClient;
use backend_rust::routes;
use backend_rust::routes::auth::AppState;
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
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

    let state = Arc::new(AppState { supabase, port });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/api/v1/auth/verify-license", post(routes::auth::verify_license))
        .route("/api/v1/auth/:provider/connect", get(routes::auth::oauth_connect))
        .route("/api/v1/auth/:provider/callback", get(routes::auth::oauth_callback))
        .route("/api/v1/agent/process-standard", post(routes::agent::process_standard))
        .route("/api/v1/integrations/dispatch", post(routes::integrations::dispatch_integrations))
        .route("/api/v1/integrations/status", get(routes::integrations::get_integrations_status))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("[MutexFlow Rust] Server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
