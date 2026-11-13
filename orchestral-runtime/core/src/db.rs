use crate::models::{
    AgentInfo, AgentStatus, AgentTokenRecord, ApiKeyRecord, AuditLogEntry, BlackboardCandidate,
    BlackboardConfirmed, BlackboardEndpoint, BlackboardService, FindingStatus, HITLRequest,
    HITLStatus, RiskTier, Severity, SwarmMode, VulnerabilityFinding,
};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Result};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn init<P: AsRef<Path>>(path: P) -> Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;

        // Performance pragmas
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        )?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.create_tables()?;
        Ok(db)
    }

    fn create_tables(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS api_keys (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                key_prefix TEXT NOT NULL,
                key_hash TEXT NOT NULL UNIQUE,
                scopes TEXT NOT NULL,
                created_at TEXT NOT NULL,
                last_used TEXT,
                is_active INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS agents (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                role TEXT NOT NULL,
                mode TEXT NOT NULL,
                status TEXT NOT NULL,
                current_task TEXT,
                tokens_used INTEGER NOT NULL,
                cost_usd REAL NOT NULL,
                last_seen TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS findings (
                id TEXT PRIMARY KEY,
                agent_id TEXT NOT NULL,
                target TEXT NOT NULL,
                title TEXT NOT NULL,
                severity TEXT NOT NULL,
                cvss REAL,
                cve TEXT,
                description TEXT NOT NULL,
                evidence TEXT,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS hitl_requests (
                id TEXT PRIMARY KEY,
                agent_id TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                target TEXT NOT NULL,
                command_preview TEXT NOT NULL,
                risk_tier TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS blackboard_services (
                target TEXT NOT NULL,
                port INTEGER NOT NULL,
                service_name TEXT NOT NULL,
                banner TEXT NOT NULL,
                discovered_by TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                PRIMARY KEY (target, port)
            );

            CREATE TABLE IF NOT EXISTS blackboard_endpoints (
                url TEXT PRIMARY KEY,
                method TEXT NOT NULL,
                status_code INTEGER NOT NULL,
                discovered_by TEXT NOT NULL,
                timestamp TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS blackboard_candidates (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                target TEXT NOT NULL,
                severity TEXT NOT NULL,
                evidence TEXT NOT NULL,
                source_agent TEXT NOT NULL,
                status TEXT NOT NULL,
                timestamp TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS blackboard_confirmed (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                target TEXT NOT NULL,
                severity TEXT NOT NULL,
                cvss_score REAL,
                confirmed_by TEXT NOT NULL,
                timestamp TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS agent_tokens (
                token_id TEXT PRIMARY KEY,
                agent_id TEXT NOT NULL,
                role TEXT NOT NULL,
                target_scope TEXT NOT NULL,
                allowed_tools TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                created_at TEXT NOT NULL,
                revoked_at TEXT
            );

            CREATE TABLE IF NOT EXISTS engagement_audit_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                actor TEXT NOT NULL,
                action TEXT NOT NULL,
                details TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                prev_hash TEXT,
                entry_hash TEXT
            );
            ",
        )?;

        // Migrations for databases created before these columns existed.
        let _ = conn.execute(
            "ALTER TABLE agent_tokens ADD COLUMN revoked_at TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE engagement_audit_log ADD COLUMN prev_hash TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE engagement_audit_log ADD COLUMN entry_hash TEXT",
            [],
        );

        conn.execute_batch(
            "
            CREATE INDEX IF NOT EXISTS idx_findings_dedup
                ON findings(target, title, agent_id);
            CREATE INDEX IF NOT EXISTS idx_agent_tokens_agent
                ON agent_tokens(agent_id);

            CREATE TRIGGER IF NOT EXISTS engagement_audit_no_update
            BEFORE UPDATE ON engagement_audit_log
            BEGIN
                SELECT RAISE(ABORT, 'engagement audit log is append-only');
            END;

            CREATE TRIGGER IF NOT EXISTS engagement_audit_no_delete
            BEFORE DELETE ON engagement_audit_log
            BEGIN
                SELECT RAISE(ABORT, 'engagement audit log is append-only');
            END;
            ",
        )?;
        Ok(())
    }

    // --- API Keys ---
    pub fn save_api_key(&self, record: &ApiKeyRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let scopes_json = serde_json::to_string(&record.scopes).unwrap_or_else(|_| "[]".to_string());
        conn.execute(
            "INSERT OR REPLACE INTO api_keys (id, name, key_prefix, key_hash, scopes, created_at, last_used, is_active)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                record.id.to_string(),
                record.name,
                record.key_prefix,
                record.key_hash,
                scopes_json,
                record.created_at.to_rfc3339(),
                record.last_used.map(|dt| dt.to_rfc3339()),
                if record.is_active { 1 } else { 0 },
            ],
        )?;
        Ok(())
    }

    pub fn get_api_key_by_hash(&self, hash: &str) -> Result<Option<ApiKeyRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, key_prefix, key_hash, scopes, created_at, last_used, is_active
             FROM api_keys WHERE key_hash = ?1",
        )?;
        let mut rows = stmt.query(params![hash])?;
        if let Some(row) = rows.next()? {
            let id_str: String = row.get(0)?;
            let scopes_str: String = row.get(4)?;
            let created_at_str: String = row.get(5)?;
            let last_used_str: Option<String> = row.get(6)?;
            let is_active_int: i32 = row.get(7)?;

            let id = Uuid::parse_str(&id_str).unwrap_or_default();
            let scopes: Vec<String> = serde_json::from_str(&scopes_str).unwrap_or_default();
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let last_used = last_used_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .ok()
            });

            Ok(Some(ApiKeyRecord {
                id,
                name: row.get(1)?,
                key_prefix: row.get(2)?,
                key_hash: row.get(3)?,
                scopes,
                created_at,
                last_used,
                is_active: is_active_int == 1,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn update_key_last_used(&self, hash: &str, when: DateTime<Utc>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE api_keys SET last_used = ?1 WHERE key_hash = ?2",
            params![when.to_rfc3339(), hash],
        )?;
        Ok(())
    }

    pub fn list_api_keys(&self) -> Result<Vec<ApiKeyRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, key_prefix, key_hash, scopes, created_at, last_used, is_active FROM api_keys",
        )?;
        let rows = stmt.query_map([], |row| {
            let id_str: String = row.get(0)?;
            let scopes_str: String = row.get(4)?;
            let created_at_str: String = row.get(5)?;
            let last_used_str: Option<String> = row.get(6)?;
            let is_active_int: i32 = row.get(7)?;

            let id = Uuid::parse_str(&id_str).unwrap_or_default();
            let scopes: Vec<String> = serde_json::from_str(&scopes_str).unwrap_or_default();
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let last_used = last_used_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .ok()
            });

            Ok(ApiKeyRecord {
                id,
                name: row.get(1)?,
                key_prefix: row.get(2)?,
                key_hash: row.get(3)?,
                scopes,
                created_at,
                last_used,
                is_active: is_active_int == 1,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            if let Ok(rec) = r {
                list.push(rec);
            }
        }
        Ok(list)
    }

    // --- Agent Tokens ---
    pub fn save_agent_token(&self, record: &AgentTokenRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let tools_json = serde_json::to_string(&record.allowed_tools)
            .unwrap_or_else(|_| "[]".to_string());
        conn.execute(
            "INSERT OR REPLACE INTO agent_tokens
             (token_id, agent_id, role, target_scope, allowed_tools, expires_at, created_at, revoked_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                record.jti,
                record.agent_id,
                record.role,
                record.target_scope,
                tools_json,
                record.expires_at.to_rfc3339(),
                record.created_at.to_rfc3339(),
                record.revoked_at.map(|dt| dt.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    pub fn get_agent_token(&self, jti: &str) -> Result<Option<AgentTokenRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT token_id, agent_id, role, target_scope, allowed_tools, expires_at, created_at, revoked_at
             FROM agent_tokens WHERE token_id = ?1",
        )?;
        let mut rows = stmt.query(params![jti])?;
        if let Some(row) = rows.next()? {
            let tools_str: String = row.get(4)?;
            let expires_str: String = row.get(5)?;
            let created_str: String = row.get(6)?;
            let revoked_str: Option<String> = row.get(7)?;

            let allowed_tools: Vec<String> =
                serde_json::from_str(&tools_str).unwrap_or_default();
            let expires_at = DateTime::parse_from_rfc3339(&expires_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let created_at = DateTime::parse_from_rfc3339(&created_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let revoked_at = revoked_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .ok()
            });

            Ok(Some(AgentTokenRecord {
                jti: row.get(0)?,
                agent_id: row.get(1)?,
                role: row.get(2)?,
                target_scope: row.get(3)?,
                allowed_tools,
                expires_at,
                created_at,
                revoked_at,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn revoke_agent_token(&self, jti: &str, when: DateTime<Utc>) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute(
            "UPDATE agent_tokens SET revoked_at = ?1 WHERE token_id = ?2 AND revoked_at IS NULL",
            params![when.to_rfc3339(), jti],
        )?;
        Ok(affected > 0)
    }

    // --- Agents ---
    pub fn save_agent(&self, agent: &AgentInfo) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let mode_str = serde_json::to_string(&agent.mode).unwrap_or_default().replace('\"', "");
        let status_str = serde_json::to_string(&agent.status).unwrap_or_default().replace('\"', "");
        conn.execute(
            "INSERT OR REPLACE INTO agents (id, name, role, mode, status, current_task, tokens_used, cost_usd, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                agent.id,
                agent.name,
                agent.role,
                mode_str,
                status_str,
                agent.current_task,
                agent.tokens_used as i64,
                agent.cost_usd,
                agent.last_seen.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_agents(&self) -> Result<Vec<AgentInfo>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, role, mode, status, current_task, tokens_used, cost_usd, last_seen FROM agents",
        )?;
        let rows = stmt.query_map([], |row| {
            let mode_str: String = row.get(3)?;
            let status_str: String = row.get(4)?;
            let tokens_i64: i64 = row.get(6)?;
            let last_seen_str: String = row.get(8)?;

            let mode: SwarmMode = serde_json::from_str(&format!("\"{}\"", mode_str))
                .unwrap_or(SwarmMode::Coordinated);
            let status: AgentStatus = serde_json::from_str(&format!("\"{}\"", status_str))
                .unwrap_or(AgentStatus::Idle);
            let last_seen = DateTime::parse_from_rfc3339(&last_seen_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(AgentInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                role: row.get(2)?,
                mode,
                status,
                current_task: row.get(5)?,
                tokens_used: tokens_i64 as u64,
                cost_usd: row.get(7)?,
                last_seen,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            if let Ok(agent) = r {
                list.push(agent);
            }
        }
        Ok(list)
    }

    // --- Findings ---
    pub fn save_finding(&self, f: &VulnerabilityFinding) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        // Deduplicate by (target, title, agent_id) so repeated runs do not
        // inflate the findings table.
        let existing: i64 = conn.query_row(
            "SELECT COUNT(*) FROM findings WHERE target = ?1 AND title = ?2 AND agent_id = ?3",
            params![f.target, f.title, f.agent_id],
            |row| row.get(0),
        )?;
        if existing > 0 {
            return Ok(());
        }

        let sev_str = serde_json::to_string(&f.severity).unwrap_or_default().replace('\"', "");
        let status_str = serde_json::to_string(&f.status).unwrap_or_default().replace('\"', "");
        conn.execute(
            "INSERT OR REPLACE INTO findings (id, agent_id, target, title, severity, cvss, cve, description, evidence, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                f.id.to_string(),
                f.agent_id,
                f.target,
                f.title,
                sev_str,
                f.cvss,
                f.cve,
                f.description,
                f.evidence,
                status_str,
                f.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_findings(&self) -> Result<Vec<VulnerabilityFinding>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, agent_id, target, title, severity, cvss, cve, description, evidence, status, created_at FROM findings",
        )?;
        let rows = stmt.query_map([], |row| {
            let id_str: String = row.get(0)?;
            let sev_str: String = row.get(4)?;
            let status_str: String = row.get(9)?;
            let created_at_str: String = row.get(10)?;

            let id = Uuid::parse_str(&id_str).unwrap_or_default();
            let severity: Severity = serde_json::from_str(&format!("\"{}\"", sev_str))
                .unwrap_or(Severity::Info);
            let status: FindingStatus = serde_json::from_str(&format!("\"{}\"", status_str))
                .unwrap_or(FindingStatus::Discovered);
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(VulnerabilityFinding {
                id,
                agent_id: row.get(1)?,
                target: row.get(2)?,
                title: row.get(3)?,
                severity,
                cvss: row.get(5)?,
                cve: row.get(6)?,
                description: row.get(7)?,
                evidence: row.get(8)?,
                status,
                created_at,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            if let Ok(f) = r {
                list.push(f);
            }
        }
        Ok(list)
    }

    // --- HITL Requests ---
    pub fn save_hitl(&self, h: &HITLRequest) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let tier_str = serde_json::to_string(&h.risk_tier).unwrap_or_default().replace('\"', "");
        let status_str = serde_json::to_string(&h.status).unwrap_or_default().replace('\"', "");
        conn.execute(
            "INSERT OR REPLACE INTO hitl_requests (id, agent_id, tool_name, target, command_preview, risk_tier, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                h.id.to_string(),
                h.agent_id,
                h.tool_name,
                h.target,
                h.command_preview,
                tier_str,
                status_str,
                h.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_hitl(&self, id: Uuid) -> Result<Option<HITLRequest>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, agent_id, tool_name, target, command_preview, risk_tier, status, created_at
             FROM hitl_requests WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id.to_string()])?;
        if let Some(row) = rows.next()? {
            let id_str: String = row.get(0)?;
            let tier_str: String = row.get(5)?;
            let status_str: String = row.get(6)?;
            let created_at_str: String = row.get(7)?;

            let id = Uuid::parse_str(&id_str).unwrap_or_default();
            let risk_tier: RiskTier = serde_json::from_str(&format!("\"{}\"", tier_str))
                .unwrap_or(RiskTier::HighImpact);
            let status: HITLStatus = serde_json::from_str(&format!("\"{}\"", status_str))
                .unwrap_or(HITLStatus::Pending);
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(Some(HITLRequest {
                id,
                agent_id: row.get(1)?,
                tool_name: row.get(2)?,
                target: row.get(3)?,
                command_preview: row.get(4)?,
                risk_tier,
                status,
                created_at,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_pending_hitl(&self) -> Result<Vec<HITLRequest>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, agent_id, tool_name, target, command_preview, risk_tier, status, created_at
             FROM hitl_requests WHERE status = 'pending'",
        )?;
        let rows = stmt.query_map([], |row| {
            let id_str: String = row.get(0)?;
            let tier_str: String = row.get(5)?;
            let status_str: String = row.get(6)?;
            let created_at_str: String = row.get(7)?;

            let id = Uuid::parse_str(&id_str).unwrap_or_default();
            let risk_tier: RiskTier = serde_json::from_str(&format!("\"{}\"", tier_str))
                .unwrap_or(RiskTier::HighImpact);
            let status: HITLStatus = serde_json::from_str(&format!("\"{}\"", status_str))
                .unwrap_or(HITLStatus::Pending);
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(HITLRequest {
                id,
                agent_id: row.get(1)?,
                tool_name: row.get(2)?,
                target: row.get(3)?,
                command_preview: row.get(4)?,
                risk_tier,
                status,
                created_at,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            if let Ok(h) = r {
                list.push(h);
            }
        }
        Ok(list)
    }

    pub fn update_hitl_status(&self, id: Uuid, status: HITLStatus) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let status_str = serde_json::to_string(&status).unwrap_or_default().replace('\"', "");
        let affected = conn.execute(
            "UPDATE hitl_requests SET status = ?1 WHERE id = ?2",
            params![status_str, id.to_string()],
        )?;
        Ok(affected > 0)
    }

    // --- Blackboard Services ---
    pub fn save_blackboard_service(&self, svc: &BlackboardService) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO blackboard_services (target, port, service_name, banner, discovered_by, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                svc.target,
                svc.port as i64,
                svc.service_name,
                svc.banner,
                svc.discovered_by,
                svc.timestamp.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_blackboard_services(&self, target_filter: Option<&str>) -> Result<Vec<BlackboardService>> {
        let conn = self.conn.lock().unwrap();
        let mut list = Vec::new();
        if let Some(target) = target_filter {
            let mut stmt = conn.prepare(
                "SELECT target, port, service_name, banner, discovered_by, timestamp
                 FROM blackboard_services WHERE target = ?1 ORDER BY port ASC",
            )?;
            let rows = stmt.query_map(params![target], Self::map_blackboard_service)?;
            for r in rows.flatten() {
                list.push(r);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT target, port, service_name, banner, discovered_by, timestamp
                 FROM blackboard_services ORDER BY target, port ASC",
            )?;
            let rows = stmt.query_map([], Self::map_blackboard_service)?;
            for r in rows.flatten() {
                list.push(r);
            }
        }
        Ok(list)
    }

    fn map_blackboard_service(row: &rusqlite::Row) -> rusqlite::Result<BlackboardService> {
        let port_i64: i64 = row.get(1)?;
        let ts_str: String = row.get(5)?;
        let ts = DateTime::parse_from_rfc3339(&ts_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        Ok(BlackboardService {
            target: row.get(0)?,
            port: port_i64 as u16,
            service_name: row.get(2)?,
            banner: row.get(3)?,
            discovered_by: row.get(4)?,
            timestamp: ts,
        })
    }

    // --- Blackboard Endpoints ---
    pub fn save_blackboard_endpoint(&self, ep: &BlackboardEndpoint) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO blackboard_endpoints (url, method, status_code, discovered_by, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                ep.url,
                ep.method,
                ep.status_code as i64,
                ep.discovered_by,
                ep.timestamp.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_blackboard_endpoints(&self) -> Result<Vec<BlackboardEndpoint>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT url, method, status_code, discovered_by, timestamp FROM blackboard_endpoints ORDER BY timestamp DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let code_i64: i64 = row.get(2)?;
            let ts_str: String = row.get(4)?;
            let ts = DateTime::parse_from_rfc3339(&ts_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(BlackboardEndpoint {
                url: row.get(0)?,
                method: row.get(1)?,
                status_code: code_i64 as u16,
                discovered_by: row.get(3)?,
                timestamp: ts,
            })
        })?;
        let mut list = Vec::new();
        for r in rows.flatten() {
            list.push(r);
        }
        Ok(list)
    }

    // --- Blackboard Candidates ---
    pub fn save_blackboard_candidate(&self, c: &BlackboardCandidate) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let existing: i64 = conn.query_row(
            "SELECT COUNT(*) FROM blackboard_candidates WHERE target = ?1 AND title = ?2",
            params![c.target, c.title],
            |row| row.get(0),
        )?;
        if existing > 0 {
            return Ok(());
        }
        conn.execute(
            "INSERT OR REPLACE INTO blackboard_candidates (id, title, target, severity, evidence, source_agent, status, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                c.id,
                c.title,
                c.target,
                c.severity,
                c.evidence,
                c.source_agent,
                c.status,
                c.timestamp.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_blackboard_candidates(&self) -> Result<Vec<BlackboardCandidate>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, target, severity, evidence, source_agent, status, timestamp
             FROM blackboard_candidates ORDER BY timestamp DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let ts_str: String = row.get(7)?;
            let ts = DateTime::parse_from_rfc3339(&ts_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(BlackboardCandidate {
                id: row.get(0)?,
                title: row.get(1)?,
                target: row.get(2)?,
                severity: row.get(3)?,
                evidence: row.get(4)?,
                source_agent: row.get(5)?,
                status: row.get(6)?,
                timestamp: ts,
            })
        })?;
        let mut list = Vec::new();
        for r in rows.flatten() {
            list.push(r);
        }
        Ok(list)
    }

    // --- Blackboard Confirmed ---
    pub fn save_blackboard_confirmed(&self, c: &BlackboardConfirmed) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let existing: i64 = conn.query_row(
            "SELECT COUNT(*) FROM blackboard_confirmed WHERE target = ?1 AND title = ?2",
            params![c.target, c.title],
            |row| row.get(0),
        )?;
        if existing > 0 {
            return Ok(());
        }
        conn.execute(
            "INSERT OR REPLACE INTO blackboard_confirmed (id, title, target, severity, cvss_score, confirmed_by, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                c.id,
                c.title,
                c.target,
                c.severity,
                c.cvss_score,
                c.confirmed_by,
                c.timestamp.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_blackboard_confirmed(&self) -> Result<Vec<BlackboardConfirmed>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, target, severity, cvss_score, confirmed_by, timestamp
             FROM blackboard_confirmed ORDER BY timestamp DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let ts_str: String = row.get(6)?;
            let ts = DateTime::parse_from_rfc3339(&ts_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(BlackboardConfirmed {
                id: row.get(0)?,
                title: row.get(1)?,
                target: row.get(2)?,
                severity: row.get(3)?,
                cvss_score: row.get(4)?,
                confirmed_by: row.get(5)?,
                timestamp: ts,
            })
        })?;
        let mut list = Vec::new();
        for r in rows.flatten() {
            list.push(r);
        }
        Ok(list)
    }

    // --- Audit Trail ---
    pub fn log_audit(&self, actor: &str, action: &str, details: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let timestamp = Utc::now().to_rfc3339();

        let prev_hash: Option<String> = conn
            .query_row(
                "SELECT entry_hash FROM engagement_audit_log ORDER BY id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?
            .flatten();

        let prev = prev_hash.clone().unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(prev.as_bytes());
        hasher.update(b"\x1f");
        hasher.update(actor.as_bytes());
        hasher.update(b"\x1f");
        hasher.update(action.as_bytes());
        hasher.update(b"\x1f");
        hasher.update(details.as_bytes());
        hasher.update(b"\x1f");
        hasher.update(timestamp.as_bytes());
        let entry_hash = hex::encode(hasher.finalize());

        conn.execute(
            "INSERT INTO engagement_audit_log
             (actor, action, details, timestamp, prev_hash, entry_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![actor, action, details, timestamp, prev_hash, entry_hash],
        )?;
        Ok(())
    }

    pub fn list_audit_logs(&self, limit: usize) -> Result<Vec<AuditLogEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, actor, action, details, timestamp, prev_hash, entry_hash
             FROM engagement_audit_log ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            let id: i64 = row.get(0)?;
            let ts_str: String = row.get(4)?;
            let ts = DateTime::parse_from_rfc3339(&ts_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(AuditLogEntry {
                id,
                actor: row.get(1)?,
                action: row.get(2)?,
                details: row.get(3)?,
                timestamp: ts,
                prev_hash: row.get(5)?,
                entry_hash: row.get(6)?,
            })
        })?;
        let mut list = Vec::new();
        for r in rows.flatten() {
            list.push(r);
        }
        Ok(list)
    }
}
