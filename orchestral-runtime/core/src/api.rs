use crate::auth::{extract_raw_key_from_parts, generate_api_key, AuthError};
use crate::models::{
    AgentInfo, BlackboardCandidate, BlackboardConfirmed, BlackboardEndpoint, BlackboardService,
    FindingStatus, HITLRequest, HITLStatus, RiskTier, Severity, SwarmMode, TelemetryEvent,
    VulnerabilityFinding,
};
use crate::scope::{target_in_scope, tool_allowed};
use crate::state::{AppState, AuthContext};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::{request::Parts, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use futures_util::{stream::Stream, SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct CreateKeyRequest {
    pub name: String,
    pub scopes: Vec<String>,
}

#[derive(Serialize)]
pub struct CreateKeyResponse {
    pub key: String,
    pub prefix: String,
    pub name: String,
    pub scopes: Vec<String>,
}

#[derive(Deserialize)]
pub struct IngestFindingRequest {
    pub agent_id: String,
    pub target: String,
    pub title: String,
    pub severity: Severity,
    pub cvss: Option<f32>,
    pub cve: Option<String>,
    pub description: String,
    pub evidence: Option<String>,
    pub status: Option<FindingStatus>,
}

#[derive(Deserialize)]
pub struct CreateHITLRequest {
    pub agent_id: String,
    pub tool_name: String,
    pub target: String,
    pub command_preview: String,
    pub risk_tier: RiskTier,
}

#[derive(Deserialize)]
pub struct CreateAgentTokenRequest {
    pub agent_id: String,
    pub role: String,
    pub target_scope: String,
    pub allowed_tools: Option<Vec<String>>,
    pub valid_seconds: Option<i64>,
}

#[derive(Deserialize)]
pub struct TargetQuery {
    pub target: Option<String>,
}

#[derive(Deserialize)]
pub struct AuditQuery {
    pub limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct WsAuthQuery {
    pub api_key: Option<String>,
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        // Authentication & Agent Tokens
        .route("/api/v1/auth/keys", post(create_key_handler).get(list_keys_handler))
        .route("/api/v1/auth/agent-token", post(create_agent_token_handler))
        .route("/api/v1/auth/agent-token/{jti}/revoke", post(revoke_agent_token_handler))
        // Swarm & Agents
        .route("/api/v1/swarm/agents", get(list_agents_handler).post(upsert_agent_handler))
        .route("/api/v1/swarm/topology", get(get_topology_handler))
        // Findings
        .route("/api/v1/findings", get(list_findings_handler).post(create_finding_handler))
        // HITL Gates
        .route("/api/v1/hitl/pending", get(list_pending_hitl_handler).post(create_hitl_handler))
        .route("/api/v1/hitl/{id}", get(get_hitl_handler))
        .route("/api/v1/hitl/{id}/approve", post(approve_hitl_handler))
        .route("/api/v1/hitl/{id}/deny", post(deny_hitl_handler))
        // Persistent Blackboard
        .route("/api/v1/blackboard/services", get(list_services_handler).post(add_service_handler))
        .route("/api/v1/blackboard/endpoints", get(list_endpoints_handler).post(add_endpoint_handler))
        .route("/api/v1/blackboard/candidates", get(list_candidates_handler).post(add_candidate_handler))
        .route("/api/v1/blackboard/confirmed", get(list_confirmed_handler).post(add_confirmed_handler))
        .route("/api/v1/blackboard/summary", get(get_blackboard_summary_handler))
        // Audit Trail
        .route("/api/v1/audit/logs", get(list_audit_logs_handler))
        // Streaming Telemetry (WebSocket & SSE for OpenCode TUI)
        .route("/api/v1/events/ws", get(ws_handler))
        .route("/api/v1/events/sse", get(sse_handler))
        .with_state(state)
}

async fn check_auth(parts: &Parts, state: &AppState, required_scope: &str) -> Result<AuthContext, AuthError> {
    let key = extract_raw_key_from_parts(parts).ok_or(AuthError::MissingHeader)?;
    state.authenticate(&key, required_scope).await
}

/// Agent tokens may only act on targets inside their issued scope.
fn enforce_target(auth: &AuthContext, target: &str) -> Result<(), AuthError> {
    if let Some(claims) = auth.agent_claims() {
        if !target_in_scope(&claims.target_scope, target) {
            return Err(AuthError::InsufficientPermissions(format!(
                "target '{}' is outside token scope '{}'",
                target, claims.target_scope
            )));
        }
    }
    Ok(())
}

/// Agent tokens may only write state for their own agent id.
fn enforce_identity(auth: &AuthContext, agent_id: &str) -> Result<(), AuthError> {
    if let Some(claims) = auth.agent_claims() {
        if claims.sub != agent_id {
            return Err(AuthError::InsufficientPermissions(format!(
                "agent token for '{}' cannot act as '{}'",
                claims.sub, agent_id
            )));
        }
    }
    Ok(())
}

/// Certain privileged operations are never available to agent tokens.
fn forbid_agent(auth: &AuthContext, action: &str) -> Result<(), AuthError> {
    if auth.agent_claims().is_some() {
        return Err(AuthError::InsufficientPermissions(format!(
            "agent tokens may not {}",
            action
        )));
    }
    Ok(())
}

fn enforce_tool(auth: &AuthContext, tool: &str) -> Result<(), AuthError> {
    if let Some(claims) = auth.agent_claims() {
        if !tool_allowed(&claims.allowed_tools, tool) {
            return Err(AuthError::InsufficientPermissions(format!(
                "tool '{}' is not allowed by token",
                tool
            )));
        }
    }
    Ok(())
}

fn in_agent_scope(auth: &AuthContext, target: &str) -> bool {
    match auth.agent_claims() {
        Some(claims) => target_in_scope(&claims.target_scope, target),
        None => true,
    }
}

async fn health_check() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "service": "orchestral-core",
        "version": "0.2.0",
        "storage": "sqlite-wal",
        "timestamp": Utc::now()
    }))
}

// --- Key Management ---
async fn create_key_handler(
    State(state): State<AppState>,
    parts: Parts,
    Json(payload): Json<CreateKeyRequest>,
) -> Result<impl IntoResponse, AuthError> {
    check_auth(&parts, &state, "admin").await?;

    let (key, prefix, _) = generate_api_key();
    let record = state
        .add_api_key(payload.name, &key, prefix.clone(), payload.scopes)
        .await
        .map_err(AuthError::Internal)?;

    Ok((
        StatusCode::CREATED,
        Json(CreateKeyResponse {
            key,
            prefix,
            name: record.name,
            scopes: record.scopes,
        }),
    ))
}

async fn list_keys_handler(
    State(state): State<AppState>,
    parts: Parts,
) -> Result<impl IntoResponse, AuthError> {
    check_auth(&parts, &state, "admin").await?;
    let keys = state.list_api_keys().await;
    Ok(Json(keys))
}

async fn create_agent_token_handler(
    State(state): State<AppState>,
    parts: Parts,
    Json(payload): Json<CreateAgentTokenRequest>,
) -> Result<impl IntoResponse, AuthError> {
    check_auth(&parts, &state, "admin").await?;

    if payload.target_scope.trim().is_empty() {
        return Err(AuthError::InsufficientPermissions(
            "target_scope must not be empty".to_string(),
        ));
    }

    let valid_seconds = payload.valid_seconds.unwrap_or(3600).clamp(60, 86_400);
    let allowed_tools = payload.allowed_tools.unwrap_or_else(|| vec!["*".to_string()]);

    match state.issue_agent_token(
        &payload.agent_id,
        &payload.role,
        &payload.target_scope,
        allowed_tools,
        valid_seconds,
    ) {
        Ok(token) => Ok(Json(json!({
            "status": "issued",
            "agent_id": payload.agent_id,
            "token": token,
            "expires_in_seconds": valid_seconds
        }))),
        Err(err) => Err(AuthError::Internal(err)),
    }
}

async fn revoke_agent_token_handler(
    State(state): State<AppState>,
    Path(jti): Path<String>,
    parts: Parts,
) -> Result<impl IntoResponse, AuthError> {
    check_auth(&parts, &state, "admin").await?;
    let revoked = state
        .revoke_agent_token(&jti)
        .await
        .map_err(AuthError::Internal)?;
    if revoked {
        Ok(Json(json!({ "jti": jti, "status": "revoked" })))
    } else {
        Err(AuthError::NotFound)
    }
}

// --- Swarm & Topology ---
async fn list_agents_handler(
    State(state): State<AppState>,
    parts: Parts,
) -> Result<impl IntoResponse, AuthError> {
    let auth = check_auth(&parts, &state, "swarm:read").await?;
    let mut agents = state.list_agents().await;
    if let Some(claims) = auth.agent_claims() {
        agents.retain(|a| a.id == claims.sub);
    }
    Ok(Json(agents))
}

async fn upsert_agent_handler(
    State(state): State<AppState>,
    parts: Parts,
    Json(payload): Json<AgentInfo>,
) -> Result<impl IntoResponse, AuthError> {
    let auth = check_auth(&parts, &state, "swarm:write").await?;
    enforce_identity(&auth, &payload.id)?;
    state
        .upsert_agent(payload)
        .await
        .map_err(AuthError::Internal)?;
    Ok(StatusCode::OK)
}

async fn get_topology_handler(
    State(state): State<AppState>,
    parts: Parts,
) -> Result<impl IntoResponse, AuthError> {
    check_auth(&parts, &state, "swarm:read").await?;
    let agents = state.list_agents().await;

    // The graph is built entirely from registered agents. There is no
    // synthetic placeholder node: an empty swarm renders as an empty graph.
    if agents.is_empty() {
        return Ok(Json(json!({
            "nodes": [],
            "edges": [],
            "agent_count": 0,
        })));
    }

    let is_coordinator = |agent: &AgentInfo| -> bool {
        agent.mode == SwarmMode::Coordinated
            || agent.role.to_lowercase().contains("coord")
            || agent.name.to_lowercase().contains("coordinator")
    };
    let primary_coordinator = agents.iter().find(|a| is_coordinator(a)).map(|a| a.id.clone());

    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    for (idx, agent) in agents.iter().enumerate() {
        let coordinator = is_coordinator(agent);
        nodes.push(json!({
            "id": agent.id,
            "type": if coordinator { "coordinatorNode" } else { "agentNode" },
            "data": {
                "label": agent.name,
                "role": agent.role,
                "status": agent.status,
                "current_task": agent.current_task,
                "tokens_used": agent.tokens_used,
                "cost_usd": agent.cost_usd,
                "mode": agent.mode,
                "last_seen": agent.last_seen,
            },
            "position": {
                "x": 100 + (idx as i32 * 220),
                "y": if coordinator { 50 } else { 200 }
            }
        }));

        // Only draw an edge when a real coordinator exists and this is not it.
        if let Some(coord_id) = primary_coordinator.as_deref() {
            if coord_id != agent.id {
                edges.push(json!({
                    "id": format!("edge-coord-{}", agent.id),
                    "source": coord_id,
                    "target": agent.id,
                    "animated": matches!(
                        agent.status,
                        crate::models::AgentStatus::Thinking
                            | crate::models::AgentStatus::RunningTool
                    )
                }));
            }
        }
    }

    Ok(Json(json!({
        "nodes": nodes,
        "edges": edges,
        "agent_count": agents.len(),
    })))
}

// --- Findings ---
async fn list_findings_handler(
    State(state): State<AppState>,
    parts: Parts,
) -> Result<impl IntoResponse, AuthError> {
    let auth = check_auth(&parts, &state, "swarm:read").await?;
    let mut findings = state.list_findings().await;
    if auth.agent_claims().is_some() {
        findings.retain(|f| in_agent_scope(&auth, &f.target));
    }
    Ok(Json(findings))
}

async fn create_finding_handler(
    State(state): State<AppState>,
    parts: Parts,
    Json(payload): Json<IngestFindingRequest>,
) -> Result<impl IntoResponse, AuthError> {
    let auth = check_auth(&parts, &state, "swarm:write").await?;
    enforce_identity(&auth, &payload.agent_id)?;
    enforce_target(&auth, &payload.target)?;

    let status = payload.status.unwrap_or(FindingStatus::Discovered);
    // Agent tokens can report observations, but only operators/triage may
    // confirm a finding.
    if auth.agent_claims().is_some() && status == FindingStatus::Confirmed {
        return Err(AuthError::InsufficientPermissions(
            "agent tokens may not confirm findings".to_string(),
        ));
    }

    let finding = VulnerabilityFinding {
        id: Uuid::new_v4(),
        agent_id: payload.agent_id,
        target: payload.target,
        title: payload.title,
        severity: payload.severity,
        cvss: payload.cvss,
        cve: payload.cve,
        description: payload.description,
        evidence: payload.evidence,
        status,
        created_at: Utc::now(),
    };

    state
        .add_finding(finding.clone())
        .await
        .map_err(AuthError::Internal)?;
    Ok((StatusCode::CREATED, Json(finding)))
}

// --- HITL Gates ---
async fn list_pending_hitl_handler(
    State(state): State<AppState>,
    parts: Parts,
) -> Result<impl IntoResponse, AuthError> {
    let auth = check_auth(&parts, &state, "swarm:read").await?;
    let mut pending = state.list_pending_hitl().await;
    if let Some(claims) = auth.agent_claims() {
        pending.retain(|h| h.agent_id == claims.sub);
    }
    Ok(Json(pending))
}

async fn get_hitl_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    parts: Parts,
) -> Result<impl IntoResponse, AuthError> {
    let auth = check_auth(&parts, &state, "swarm:read").await?;
    if let Some(req) = state.get_hitl_request(id).await {
        if let Some(claims) = auth.agent_claims() {
            if req.agent_id != claims.sub {
                return Err(AuthError::NotFound);
            }
        }
        Ok(Json(req))
    } else {
        Err(AuthError::NotFound)
    }
}

async fn create_hitl_handler(
    State(state): State<AppState>,
    parts: Parts,
    Json(payload): Json<CreateHITLRequest>,
) -> Result<impl IntoResponse, AuthError> {
    let auth = check_auth(&parts, &state, "swarm:write").await?;
    enforce_identity(&auth, &payload.agent_id)?;
    enforce_target(&auth, &payload.target)?;
    enforce_tool(&auth, &payload.tool_name)?;

    let req = HITLRequest {
        id: Uuid::new_v4(),
        agent_id: payload.agent_id,
        tool_name: payload.tool_name,
        target: payload.target,
        command_preview: payload.command_preview,
        risk_tier: payload.risk_tier,
        status: HITLStatus::Pending,
        created_at: Utc::now(),
    };

    state
        .create_hitl_request(req.clone())
        .await
        .map_err(AuthError::Internal)?;
    Ok((StatusCode::CREATED, Json(req)))
}

async fn approve_hitl_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    parts: Parts,
) -> Result<impl IntoResponse, AuthError> {
    check_auth(&parts, &state, "agent:hitl_approve").await?;
    if state.resolve_hitl(id, HITLStatus::Approved).await {
        Ok(Json(json!({ "id": id, "status": "approved" })))
    } else {
        Err(AuthError::NotFound)
    }
}

async fn deny_hitl_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    parts: Parts,
) -> Result<impl IntoResponse, AuthError> {
    check_auth(&parts, &state, "agent:hitl_approve").await?;
    if state.resolve_hitl(id, HITLStatus::Denied).await {
        Ok(Json(json!({ "id": id, "status": "denied" })))
    } else {
        Err(AuthError::NotFound)
    }
}

// --- Blackboard Handlers ---
async fn list_services_handler(
    State(state): State<AppState>,
    Query(query): Query<TargetQuery>,
    parts: Parts,
) -> Result<impl IntoResponse, AuthError> {
    let auth = check_auth(&parts, &state, "swarm:read").await?;
    let mut list = state.list_blackboard_services(query.target.as_deref()).await;
    if auth.agent_claims().is_some() {
        list.retain(|s| in_agent_scope(&auth, &s.target));
    }
    Ok(Json(list))
}

async fn add_service_handler(
    State(state): State<AppState>,
    parts: Parts,
    Json(payload): Json<BlackboardService>,
) -> Result<impl IntoResponse, AuthError> {
    let auth = check_auth(&parts, &state, "swarm:write").await?;
    enforce_target(&auth, &payload.target)?;
    if let Some(claims) = auth.agent_claims() {
        if payload.discovered_by != claims.sub {
            return Err(AuthError::InsufficientPermissions(
                "discovered_by must match the agent token subject".to_string(),
            ));
        }
    }
    state
        .add_blackboard_service(payload)
        .await
        .map_err(AuthError::Internal)?;
    Ok(StatusCode::CREATED)
}

async fn list_endpoints_handler(
    State(state): State<AppState>,
    parts: Parts,
) -> Result<impl IntoResponse, AuthError> {
    let auth = check_auth(&parts, &state, "swarm:read").await?;
    let mut list = state.list_blackboard_endpoints().await;
    if auth.agent_claims().is_some() {
        list.retain(|e| in_agent_scope(&auth, &e.url));
    }
    Ok(Json(list))
}

async fn add_endpoint_handler(
    State(state): State<AppState>,
    parts: Parts,
    Json(payload): Json<BlackboardEndpoint>,
) -> Result<impl IntoResponse, AuthError> {
    let auth = check_auth(&parts, &state, "swarm:write").await?;
    enforce_target(&auth, &payload.url)?;
    state
        .add_blackboard_endpoint(payload)
        .await
        .map_err(AuthError::Internal)?;
    Ok(StatusCode::CREATED)
}

async fn list_candidates_handler(
    State(state): State<AppState>,
    parts: Parts,
) -> Result<impl IntoResponse, AuthError> {
    let auth = check_auth(&parts, &state, "swarm:read").await?;
    let mut list = state.list_blackboard_candidates().await;
    if auth.agent_claims().is_some() {
        list.retain(|c| in_agent_scope(&auth, &c.target));
    }
    Ok(Json(list))
}

async fn add_candidate_handler(
    State(state): State<AppState>,
    parts: Parts,
    Json(payload): Json<BlackboardCandidate>,
) -> Result<impl IntoResponse, AuthError> {
    let auth = check_auth(&parts, &state, "swarm:write").await?;
    enforce_target(&auth, &payload.target)?;
    if let Some(claims) = auth.agent_claims() {
        if payload.source_agent != claims.sub {
            return Err(AuthError::InsufficientPermissions(
                "source_agent must match the agent token subject".to_string(),
            ));
        }
    }
    state
        .add_blackboard_candidate(payload)
        .await
        .map_err(AuthError::Internal)?;
    Ok(StatusCode::CREATED)
}

async fn list_confirmed_handler(
    State(state): State<AppState>,
    parts: Parts,
) -> Result<impl IntoResponse, AuthError> {
    let auth = check_auth(&parts, &state, "swarm:read").await?;
    let mut list = state.list_blackboard_confirmed().await;
    if auth.agent_claims().is_some() {
        list.retain(|c| in_agent_scope(&auth, &c.target));
    }
    Ok(Json(list))
}

async fn add_confirmed_handler(
    State(state): State<AppState>,
    parts: Parts,
    Json(payload): Json<BlackboardConfirmed>,
) -> Result<impl IntoResponse, AuthError> {
    let auth = check_auth(&parts, &state, "swarm:write").await?;
    forbid_agent(&auth, "confirm findings")?;
    enforce_target(&auth, &payload.target)?;
    state
        .add_blackboard_confirmed(payload)
        .await
        .map_err(AuthError::Internal)?;
    Ok(StatusCode::CREATED)
}

async fn get_blackboard_summary_handler(
    State(state): State<AppState>,
    Query(query): Query<TargetQuery>,
    parts: Parts,
) -> Result<impl IntoResponse, AuthError> {
    let auth = check_auth(&parts, &state, "swarm:read").await?;
    let mut services = state.list_blackboard_services(None).await;
    let mut endpoints = state.list_blackboard_endpoints().await;
    let mut candidates = state.list_blackboard_candidates().await;
    let mut confirmed = state.list_blackboard_confirmed().await;

    if let Some(target) = query.target.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        services.retain(|s| s.target == target);
        endpoints.retain(|e| e.url.contains(target));
        candidates.retain(|c| c.target == target);
        confirmed.retain(|c| c.target == target);
    }

    if auth.agent_claims().is_some() {
        services.retain(|s| in_agent_scope(&auth, &s.target));
        endpoints.retain(|e| in_agent_scope(&auth, &e.url));
        candidates.retain(|c| in_agent_scope(&auth, &c.target));
        confirmed.retain(|c| in_agent_scope(&auth, &c.target));
    }

    Ok(Json(json!({
        "total_services": services.len(),
        "total_endpoints": endpoints.len(),
        "candidates": candidates.len(),
        "confirmed_vulnerabilities": confirmed.len(),
        "services": services,
        "endpoints": endpoints,
        "candidates_list": candidates,
        "confirmed_list": confirmed
    })))
}

// --- Audit Log Handler ---
async fn list_audit_logs_handler(
    State(state): State<AppState>,
    Query(query): Query<AuditQuery>,
    parts: Parts,
) -> Result<impl IntoResponse, AuthError> {
    check_auth(&parts, &state, "admin").await?;
    let limit = query.limit.unwrap_or(50).clamp(1, 1000);
    let logs = state.list_audit_logs(limit).await;
    Ok(Json(logs))
}

// --- WebSocket Streaming ---
async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsAuthQuery>,
    parts: Parts,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let raw_key = extract_raw_key_from_parts(&parts)
        .or(query.api_key)
        .unwrap_or_default();
    if state.validate_key(&raw_key, "swarm:read").await.is_none() {
        return (StatusCode::UNAUTHORIZED, "Invalid API key").into_response();
    }

    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.tx.subscribe();

    tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if let Ok(json_str) = serde_json::to_string(&msg) {
                if sender.send(Message::Text(json_str.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    while let Some(Ok(_)) = receiver.next().await {}
}

// --- Server-Sent Events (SSE) Streaming for OpenCode TUI ---
async fn sse_handler(
    Query(query): Query<WsAuthQuery>,
    parts: Parts,
    State(state): State<AppState>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    // Prefer the header; keep the query parameter as a compatibility fallback.
    let raw_key = extract_raw_key_from_parts(&parts)
        .or(query.api_key)
        .unwrap_or_default();
    if state.validate_key(&raw_key, "swarm:read").await.is_none() {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let rx = state.tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|msg| async move {
        match msg {
            Ok(event) => {
                let (event_name, data) = match &event {
                    TelemetryEvent::AgentUpdated(a) => ("agent.updated", serde_json::to_string(a).unwrap_or_default()),
                    TelemetryEvent::FindingReported(f) => ("finding.reported", serde_json::to_string(f).unwrap_or_default()),
                    TelemetryEvent::HITLRequested(h) => ("hitl.pending", serde_json::to_string(h).unwrap_or_default()),
                    TelemetryEvent::HITLResolved { id, status } => ("hitl.resolved", json!({ "id": id, "status": status }).to_string()),
                    TelemetryEvent::BlackboardUpdated { category, item } => ("blackboard.updated", json!({ "category": category, "item": item }).to_string()),
                    TelemetryEvent::SystemLog { level, message, timestamp } => ("system.log", json!({ "level": level, "message": message, "timestamp": timestamp }).to_string()),
                };

                Some(Ok(Event::default().event(event_name).data(data)))
            }
            Err(_) => None,
        }
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}
