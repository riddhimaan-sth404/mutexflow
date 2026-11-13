use backend_rust::services::jcode_manager::JcodeManager;
use std::path::PathBuf;

#[tokio::test]
async fn isolated_jcode_runtime_smoke() {
    if std::env::var("JCODE_RUNTIME_SMOKE").ok().as_deref() != Some("1") {
        return;
    }

    std::env::set_var("JCODE_AUTOSTART", "true");
    let binary = std::env::var("JCODE_BINARY").expect("JCODE_BINARY must point to the built JCode executable");
    std::env::set_var("JCODE_BINARY", binary);

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("backend has repository parent")
        .to_path_buf();
    let manager = JcodeManager::new(root);
    manager.start().expect("JCode Harness starts in isolated home");
    assert!(manager.status().connected, "Harness API should answer health ping");

    let created = manager.create_session(".").await.expect("create isolated session");
    let session_id = created["session_id"].as_str().expect("session id").to_string();
    assert_eq!(created["tools_enabled"], 0);

    let sessions = manager.sessions().await.expect("list sessions");
    assert!(sessions.as_array().expect("sessions array").iter()
        .any(|session| session["session_id"].as_str() == Some(&session_id)));

    let cancelled = manager.cancel(&session_id).await.expect("cancel idle session");
    assert_eq!(cancelled["status"], "cancellation_requested");
}
