use axum::{
    http::{header::AUTHORIZATION, header::CONTENT_TYPE, HeaderName, Method},
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
use tower_http::cors::{AllowOrigin, CorsLayer};
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
    // Resolve or create the loopback API token before any consumer needs it.
    let _api_token = backend_rust::routes::auth::ensure_local_api_token(&root);

    let orchestral = Arc::new(OrchestralManager::new(root.clone()));
    let jcode = Arc::new(backend_rust::services::jcode_manager::JcodeManager::new(root, orchestral.clone()));
    // JCode startup blocks on process spawn and a socket-connect loop; keep it
    // off the async reactor while the Orchestral runtime starts concurrently.
    let jcode_for_startup = jcode.clone();
    let jcode_startup = tokio::task::spawn_blocking(move || jcode_for_startup.start());
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        shutdown_signal().await;
        let _ = shutdown_tx.send(true);
    });

    tokio::select! {
        result = orchestral.start() => {
            if let Err(error) = result {
                tracing::error!("[Orchestral] Runtime startup failed: {}", error);
            }
        }
        changed = shutdown_rx.changed() => {
            if changed.is_ok() {
                info!("Shutdown signal received during Orchestral startup");
            }
            orchestral.shutdown().await;
            return Ok(());
        }
    }

    if let Err(error) = jcode_startup.await.unwrap_or_else(|error| Err(format!("join JCode startup task: {error}"))) {
        tracing::warn!("[JCode] Runtime startup skipped: {}", error);
    }

    let state = Arc::new(AppState { supabase, port, orchestral: orchestral.clone(), jcode: jcode.clone() });

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
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([CONTENT_TYPE, AUTHORIZATION, HeaderName::from_static("x-mutexflow-token")]);

    let app = Router::new()
        .route("/health", get(|| async { axum::Json(serde_json::json!({ "status": "ok", "service": "mutexflow-backend" })) }))
        .route("/api/v1/auth/verify-license", post(routes::auth::verify_license))
        .route("/api/v1/auth/:provider/connect", get(routes::auth::oauth_connect))
        .route("/api/v1/auth/:provider/callback", get(routes::auth::oauth_callback))
        .route("/api/v1/agent/process-standard", post(routes::agent::process_standard))
        .route("/api/v1/integrations/dispatch", post(routes::integrations::dispatch_integrations))
        .route("/api/v1/integrations/status", get(routes::integrations::get_integrations_status))
        .nest("/api/v1/orchestral", routes::orchestral::router().layer(axum::middleware::from_fn(routes::auth::require_local_access)))
        .nest("/api/v1/jcode", routes::jcode::router().layer(axum::middleware::from_fn(routes::auth::require_local_access)))
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
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.wait_for(|received| *received).await;
            info!("Shutdown signal received");
        })
        .await;

    orchestral.shutdown().await;
    server_result?;
    Ok(())
}

#[cfg(unix)]
async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};

    let mut terminate = signal(SignalKind::terminate())
        .expect("failed to install SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
