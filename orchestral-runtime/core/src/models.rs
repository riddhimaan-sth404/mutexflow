use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SwarmMode {
    Coordinated,
    Independent,
    Supervised,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Thinking,
    RunningTool,
    AwaitingApproval,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub role: String,
    pub mode: SwarmMode,
    pub status: AgentStatus,
    pub current_task: Option<String>,
    pub tokens_used: u64,
    pub cost_usd: f64,
    pub last_seen: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FindingStatus {
    Discovered,
    Validating,
    Confirmed,
    FalsePositive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VulnerabilityFinding {
    pub id: Uuid,
    pub agent_id: String,
    pub target: String,
    pub title: String,
    pub severity: Severity,
    pub cvss: Option<f32>,
    pub cve: Option<String>,
    pub description: String,
    pub evidence: Option<String>,
    pub status: FindingStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskTier {
    Passive,
    Active,
    HighImpact,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HITLStatus {
    Pending,
    Approved,
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HITLRequest {
    pub id: Uuid,
    pub agent_id: String,
    pub tool_name: String,
    pub target: String,
    pub command_preview: String,
    pub risk_tier: RiskTier,
    pub status: HITLStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyRecord {
    pub id: Uuid,
    pub name: String,
    pub key_prefix: String,
    /// Never serialize key hashes in API responses.
    #[serde(skip_serializing)]
    pub key_hash: String,
    pub scopes: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub last_used: Option<DateTime<Utc>>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlackboardService {
    pub target: String,
    pub port: u16,
    pub service_name: String,
    pub banner: String,
    pub discovered_by: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlackboardEndpoint {
    pub url: String,
    pub method: String,
    pub status_code: u16,
    pub discovered_by: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlackboardCandidate {
    pub id: String,
    pub title: String,
    pub target: String,
    pub severity: String,
    pub evidence: String,
    pub source_agent: String,
    pub status: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlackboardConfirmed {
    pub id: String,
    pub title: String,
    pub target: String,
    pub severity: String,
    pub cvss_score: Option<f32>,
    pub confirmed_by: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogEntry {
    pub id: i64,
    pub actor: String,
    pub action: String,
    pub details: String,
    pub timestamp: DateTime<Utc>,
    pub prev_hash: Option<String>,
    pub entry_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTokenRecord {
    pub jti: String,
    pub agent_id: String,
    pub role: String,
    pub target_scope: String,
    pub allowed_tools: Vec<String>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTokenClaims {
    pub jti: String,
    pub sub: String,
    pub role: String,
    pub target_scope: String,
    pub allowed_tools: Vec<String>,
    pub exp: i64,
    pub iat: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
#[serde(rename_all = "snake_case")]
pub enum TelemetryEvent {
    AgentUpdated(AgentInfo),
    FindingReported(VulnerabilityFinding),
    HITLRequested(HITLRequest),
    HITLResolved { id: Uuid, status: HITLStatus },
    BlackboardUpdated { category: String, item: serde_json::Value },
    SystemLog { level: String, message: String, timestamp: DateTime<Utc> },
}
