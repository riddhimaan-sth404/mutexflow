use crate::auth::{hash_api_key, AuthError};
use crate::db::Database;
use crate::models::{
    AgentInfo, AgentTokenClaims, AgentTokenRecord, ApiKeyRecord, AuditLogEntry, BlackboardCandidate,
    BlackboardConfirmed, BlackboardEndpoint, BlackboardService, HITLRequest, HITLStatus,
    TelemetryEvent, VulnerabilityFinding,
};
use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::RngExt;
use std::path::Path;
use tokio::sync::broadcast;
use uuid::Uuid;

/// Authenticated principal for a request. Agent tokens carry their claims so
/// handlers can enforce target scope and tool restrictions server-side.
#[derive(Clone)]
pub struct AuthContext {
    pub record: ApiKeyRecord,
    pub agent: Option<AgentTokenClaims>,
}

impl AuthContext {
    pub fn agent_claims(&self) -> Option<&AgentTokenClaims> {
        self.agent.as_ref()
    }
}

#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub tx: broadcast::Sender<TelemetryEvent>,
    jwt_secret: Vec<u8>,
}

impl AppState {
    pub fn new<P: AsRef<Path>>(db_path: P) -> Self {
        let (tx, _rx) = broadcast::channel(2048);
        let db = Database::init(db_path).expect("Failed to initialize SQLite database");

        // Never ship a hard-coded signing secret. Operators should set
        // ORCHESTRAL_JWT_SECRET; otherwise generate an ephemeral secret at startup.
        let jwt_secret = match std::env::var("ORCHESTRAL_JWT_SECRET") {
            Ok(secret) if !secret.trim().is_empty() => secret.into_bytes(),
            _ => {
                let mut bytes = [0u8; 32];
                rand::rng().fill(&mut bytes);
                tracing::warn!(
                    "ORCHESTRAL_JWT_SECRET is not set; generated an ephemeral signing secret. \
                     Agent tokens will be invalid after restart."
                );
                bytes.to_vec()
            }
        };

        Self {
            db,
            tx,
            jwt_secret,
        }
    }

    // --- API Key Auth ---
    pub async fn add_api_key(
        &self,
        name: String,
        full_key: &str,
        prefix: String,
        scopes: Vec<String>,
    ) -> Result<ApiKeyRecord, String> {
        let hash = hash_api_key(full_key);
        let record = ApiKeyRecord {
            id: Uuid::new_v4(),
            name,
            key_prefix: prefix,
            key_hash: hash.clone(),
            scopes,
            created_at: Utc::now(),
            last_used: None,
            is_active: true,
        };

        self.db.save_api_key(&record).map_err(|e| e.to_string())?;
        let _ = self.db.log_audit(
            "system",
            "create_api_key",
            &format!("Key prefix: {}", record.key_prefix),
        );
        Ok(record)
    }

    /// Authenticate a raw key or agent token and enforce the required scope.
    pub async fn authenticate(
        &self,
        raw_key: &str,
        required_scope: &str,
    ) -> Result<AuthContext, AuthError> {
        if raw_key.trim().is_empty() {
            return Err(AuthError::InvalidKey);
        }

        // Agent token (JWT). It must still exist in the token table, must not
        // be revoked, and must not be expired.
        if raw_key.starts_with("agt_") || raw_key.contains('.') {
            let claims = self
                .verify_agent_token(raw_key)
                .map_err(|_| AuthError::InvalidKey)?;

            match self.db.get_agent_token(&claims.jti) {
                Ok(Some(record))
                    if record.revoked_at.is_none() && record.expires_at > Utc::now() => {}
                _ => return Err(AuthError::InvalidKey),
            }

            let scopes = vec![
                "agent".to_string(),
                "swarm:read".to_string(),
                "swarm:write".to_string(),
            ];
            if required_scope != "agent" && !scopes.iter().any(|s| s == required_scope) {
                return Err(AuthError::InsufficientPermissions(required_scope.to_string()));
            }

            let record = ApiKeyRecord {
                id: Uuid::new_v4(),
                name: format!("Agent: {}", claims.sub),
                key_prefix: format!(
                    "agt_{}...",
                    &claims.sub[..std::cmp::min(4, claims.sub.len())]
                ),
                key_hash: String::new(),
                scopes,
                created_at: Utc::now(),
                last_used: Some(Utc::now()),
                is_active: true,
            };

            return Ok(AuthContext {
                record,
                agent: Some(claims),
            });
        }

        // Standard API key validation.
        let hash = hash_api_key(raw_key);
        match self.db.get_api_key_by_hash(&hash) {
            Ok(Some(mut record)) if record.is_active => {
                let is_admin = record.scopes.iter().any(|s| s == "admin");
                if !is_admin && !record.scopes.iter().any(|s| s == required_scope) {                    return Err(AuthError::InsufficientPermissions(required_scope.to_string()));
                }
                let now = Utc::now();
                let _ = self.db.update_key_last_used(&hash, now);
                record.last_used = Some(now);
                Ok(AuthContext {
                    record,
                    agent: None,
                })
            }
            _ => Err(AuthError::InvalidKey),
        }
    }

    /// Backwards-compatible helper used by streaming handlers.
    pub async fn validate_key(&self, raw_key: &str, required_scope: &str) -> Option<ApiKeyRecord> {
        self.authenticate(raw_key, required_scope)
            .await
            .ok()
            .map(|ctx| ctx.record)
    }

    pub async fn list_api_keys(&self) -> Vec<ApiKeyRecord> {
        self.db.list_api_keys().unwrap_or_default()
    }

    // --- Agent Ephemeral JWTs ---
    pub fn issue_agent_token(
        &self,
        agent_id: &str,
        role: &str,
        target_scope: &str,
        allowed_tools: Vec<String>,
        valid_seconds: i64,
    ) -> Result<String, String> {
        let now = Utc::now();
        let claims = AgentTokenClaims {
            jti: Uuid::new_v4().to_string(),
            sub: agent_id.to_string(),
            role: role.to_string(),
            target_scope: target_scope.to_string(),
            allowed_tools: allowed_tools.clone(),
            iat: now.timestamp(),
            exp: now.timestamp() + valid_seconds,
        };

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(&self.jwt_secret),
        )
        .map_err(|e| e.to_string())?;

        let record = AgentTokenRecord {
            jti: claims.jti.clone(),
            agent_id: agent_id.to_string(),
            role: role.to_string(),
            target_scope: target_scope.to_string(),
            allowed_tools,
            expires_at: now + chrono::Duration::seconds(valid_seconds),
            created_at: now,
            revoked_at: None,
        };
        self.db
            .save_agent_token(&record)
            .map_err(|e| e.to_string())?;
        let _ = self.db.log_audit(
            agent_id,
            "issue_agent_token",
            &format!("role={} scope={}", role, target_scope),
        );

        Ok(format!("agt_{}", token))
    }

    pub fn verify_agent_token(&self, token_str: &str) -> Result<AgentTokenClaims, String> {
        let cleaned = token_str.strip_prefix("agt_").unwrap_or(token_str);
        decode::<AgentTokenClaims>(
            cleaned,
            &DecodingKey::from_secret(&self.jwt_secret),
            &Validation::default(),
        )
        .map(|data| data.claims)
        .map_err(|e| e.to_string())
    }

    pub async fn revoke_agent_token(&self, jti: &str) -> Result<bool, String> {
        self.db
            .revoke_agent_token(jti, Utc::now())
            .map_err(|e| e.to_string())
    }

    // --- Agents ---
    pub async fn upsert_agent(&self, agent: AgentInfo) -> Result<(), String> {
        self.db.save_agent(&agent).map_err(|e| e.to_string())?;
        let _ = self.tx.send(TelemetryEvent::AgentUpdated(agent));
        Ok(())
    }

    pub async fn list_agents(&self) -> Vec<AgentInfo> {
        self.db.list_agents().unwrap_or_default()
    }

    // --- Findings ---
    pub async fn add_finding(&self, finding: VulnerabilityFinding) -> Result<(), String> {
        self.db
            .save_finding(&finding)
            .map_err(|e| e.to_string())?;
        let _ = self.db.log_audit(
            &finding.agent_id,
            "report_finding",
            &format!("Title: {} | Severity: {:?}", finding.title, finding.severity),
        );
        let _ = self.tx.send(TelemetryEvent::FindingReported(finding));
        Ok(())
    }

    pub async fn list_findings(&self) -> Vec<VulnerabilityFinding> {
        self.db.list_findings().unwrap_or_default()
    }

    // --- HITL Requests ---
    pub async fn create_hitl_request(&self, request: HITLRequest) -> Result<(), String> {
        self.db.save_hitl(&request).map_err(|e| e.to_string())?;
        let _ = self.db.log_audit(
            &request.agent_id,
            "hitl_requested",
            &format!("Tool: {} | Target: {}", request.tool_name, request.target),
        );
        let _ = self.tx.send(TelemetryEvent::HITLRequested(request));
        Ok(())
    }

    pub async fn get_hitl_request(&self, id: Uuid) -> Option<HITLRequest> {
        self.db.get_hitl(id).unwrap_or(None)
    }

    pub async fn list_pending_hitl(&self) -> Vec<HITLRequest> {
        self.db.list_pending_hitl().unwrap_or_default()
    }

    pub async fn resolve_hitl(&self, id: Uuid, status: HITLStatus) -> bool {
        if let Ok(updated) = self.db.update_hitl_status(id, status.clone()) {
            if updated {
                let _ = self.db.log_audit(
                    "operator",
                    "hitl_resolved",
                    &format!("ID: {} | Status: {:?}", id, status),
                );
                let _ = self.tx.send(TelemetryEvent::HITLResolved { id, status });
                return true;
            }
        }
        false
    }

    // --- Blackboard ---
    pub async fn add_blackboard_service(&self, svc: BlackboardService) -> Result<(), String> {
        self.db
            .save_blackboard_service(&svc)
            .map_err(|e| e.to_string())?;
        let _ = self.tx.send(TelemetryEvent::BlackboardUpdated {
            category: "service".to_string(),
            item: serde_json::to_value(&svc).unwrap_or_default(),
        });
        Ok(())
    }

    pub async fn list_blackboard_services(&self, target: Option<&str>) -> Vec<BlackboardService> {
        self.db.list_blackboard_services(target).unwrap_or_default()
    }

    pub async fn add_blackboard_endpoint(&self, ep: BlackboardEndpoint) -> Result<(), String> {
        self.db
            .save_blackboard_endpoint(&ep)
            .map_err(|e| e.to_string())?;
        let _ = self.tx.send(TelemetryEvent::BlackboardUpdated {
            category: "endpoint".to_string(),
            item: serde_json::to_value(&ep).unwrap_or_default(),
        });
        Ok(())
    }

    pub async fn list_blackboard_endpoints(&self) -> Vec<BlackboardEndpoint> {
        self.db.list_blackboard_endpoints().unwrap_or_default()
    }

    pub async fn add_blackboard_candidate(&self, c: BlackboardCandidate) -> Result<(), String> {
        self.db
            .save_blackboard_candidate(&c)
            .map_err(|e| e.to_string())?;
        let _ = self.tx.send(TelemetryEvent::BlackboardUpdated {
            category: "candidate".to_string(),
            item: serde_json::to_value(&c).unwrap_or_default(),
        });
        Ok(())
    }

    pub async fn list_blackboard_candidates(&self) -> Vec<BlackboardCandidate> {
        self.db.list_blackboard_candidates().unwrap_or_default()
    }

    pub async fn add_blackboard_confirmed(&self, c: BlackboardConfirmed) -> Result<(), String> {
        self.db
            .save_blackboard_confirmed(&c)
            .map_err(|e| e.to_string())?;
        let _ = self.tx.send(TelemetryEvent::BlackboardUpdated {
            category: "confirmed".to_string(),
            item: serde_json::to_value(&c).unwrap_or_default(),
        });
        Ok(())
    }

    pub async fn list_blackboard_confirmed(&self) -> Vec<BlackboardConfirmed> {
        self.db.list_blackboard_confirmed().unwrap_or_default()
    }

    // --- Audit Trail ---
    pub async fn list_audit_logs(&self, limit: usize) -> Vec<AuditLogEntry> {
        self.db.list_audit_logs(limit).unwrap_or_default()
    }
}
