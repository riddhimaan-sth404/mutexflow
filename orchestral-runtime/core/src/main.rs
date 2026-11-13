mod api;
mod auth;
mod db;
mod models;
mod scope;
mod state;

use api::create_router;
use auth::generate_api_key;
use state::AppState;
use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tracing::{info, warn, Level};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set tracing subscriber");

    let db_path = match env::var("ORCHESTRAL_DB_PATH") {
        Ok(p) => PathBuf::from(p),
        Err(_) => {
            let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".orchestral").join("orchestral.db")
        }
    };
    info!("Initializing persistent SQLite database at {:?}", db_path);

    let state = AppState::new(db_path);

    // Bootstrap Admin API key if needed
    let existing_keys = state.list_api_keys().await;
    let admin_key_env = env::var("ORCHESTRAL_ADMIN_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty());

    if let Some(admin_key) = admin_key_env {
        info!("Registering/ensuring admin key configured via ORCHESTRAL_ADMIN_KEY");
        let prefix: String = admin_key.chars().take(14).collect();
        if let Err(e) = state
            .add_api_key(
                "Environment Admin Key".to_string(),
                &admin_key,
                format!("{}...", prefix),
                vec![
                    "admin".to_string(),
                    "swarm:read".to_string(),
                    "swarm:write".to_string(),
                    "agent:hitl_approve".to_string(),
                    "findings:export".to_string(),
                ],
            )
            .await
        {
            warn!("Failed to persist admin key: {}", e);
        }
    } else if existing_keys.is_empty() {
        let (key, prefix, _) = generate_api_key();
        println!("\n========================================================");
        println!("[CITADEL] Orchestral Citadel Security Monitor Initialized");
        println!("[SECURITY] BOOTSTRAP ADMIN API KEY: {}", key);
        println!("   Prefix: {}", prefix);
        println!("   Keep this key safe! Use it in 'X-Orchestral-API-Key' header.");
        println!("========================================================\n");

        if let Err(e) = state
            .add_api_key(
                "Bootstrap Admin Key".to_string(),
                &key,
                prefix,
                vec![
                    "admin".to_string(),
                    "swarm:read".to_string(),
                    "swarm:write".to_string(),
                    "agent:hitl_approve".to_string(),
                    "findings:export".to_string(),
                ],
            )
            .await
        {
            warn!("Failed to persist bootstrap admin key: {}", e);
        }
    } else {
        info!(
            "Loaded {} existing API keys from SQLite storage.",
            existing_keys.len()
        );
    }

    // CORS: default to localhost/loopback only. Set ORCHESTRAL_CORS_ORIGINS to a
    // comma-separated allowlist for other deployments.
    let cors = match env::var("ORCHESTRAL_CORS_ORIGINS") {
        Ok(origins) if !origins.trim().is_empty() => {
            let parsed: Vec<axum::http::HeaderValue> = origins
                .split(',')
                .filter_map(|o| o.trim().parse().ok())
                .collect();
            CorsLayer::new()
                .allow_origin(parsed)
                .allow_methods(Any)
                .allow_headers(Any)
        }
        _ => CorsLayer::new()
            .allow_origin(AllowOrigin::predicate(|origin, _| {
                origin
                    .to_str()
                    .map(|o| {
                        o.starts_with("http://localhost")
                            || o.starts_with("http://127.0.0.1")
                            || o.starts_with("https://localhost")
                            || o.starts_with("https://127.0.0.1")
                    })
                    .unwrap_or(false)
            }))
            .allow_methods(Any)
            .allow_headers(Any),
    };

    let app = create_router(state).layer(cors);

    let port: u16 = env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8088);
    // Default to loopback. Operators who need remote access can set
    // ORCHESTRAL_BIND_ADDR explicitly (for example 0.0.0.0) behind a firewall.
    let bind_host = env::var("ORCHESTRAL_BIND_ADDR")
        .unwrap_or_else(|_| "127.0.0.1".to_string());
    let addr: SocketAddr = format!("{}:{}", bind_host, port)
        .parse()
        .unwrap_or_else(|_| SocketAddr::from(([127, 0, 0, 1], port)));

    info!("Orchestral Citadel daemon listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
