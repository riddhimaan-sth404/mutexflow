import json
import urllib.parse
import urllib.request
import urllib.error
from datetime import datetime, timezone
from typing import Dict, Any, List, Optional


class OrchestralClient:
    """
    Python client for communicating with the Orchestral Rust Control Plane
    using API keys and ephemeral agent tokens.
    """

    def __init__(self, base_url: str = "http://127.0.0.1:8088", api_key: str = ""):
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key

    def _request(self, endpoint: str, method: str = "GET", payload: Optional[Dict[str, Any]] = None) -> Any:
        url = f"{self.base_url}{endpoint}"
        headers = {
            "X-Orchestral-API-Key": self.api_key,
            "Content-Type": "application/json",
        }
        data = json.dumps(payload).encode("utf-8") if payload is not None else None
        req = urllib.request.Request(url, data=data, headers=headers, method=method)
        try:
            with urllib.request.urlopen(req, timeout=15) as resp:
                resp_data = resp.read().decode("utf-8")
                return json.loads(resp_data) if resp_data else {}
        except urllib.error.HTTPError as e:
            err_body = e.read().decode("utf-8")
            raise RuntimeError(f"Orchestral Citadel API Error ({e.code}): {err_body}") from e
        except Exception as e:
            raise RuntimeError(f"Failed to connect to Orchestral Citadel at {url}: {e}") from e

    def health(self) -> Dict[str, Any]:
        return self._request("/health")

    def get_health(self) -> Dict[str, Any]:
        return self.health()

    # --- Agent Lifecycle & Topology ---
    def register_or_update_agent(self, agent_data: Dict[str, Any]) -> Dict[str, Any]:
        return self._request("/api/v1/swarm/agents", method="POST", payload=agent_data)

    def list_agents(self) -> List[Dict[str, Any]]:
        return self._request("/api/v1/swarm/agents")

    def get_topology(self) -> Dict[str, Any]:
        return self._request("/api/v1/swarm/topology")

    # --- Findings ---
    def submit_finding(self, finding: Dict[str, Any]) -> Dict[str, Any]:
        return self._request("/api/v1/findings", method="POST", payload=finding)

    def list_findings(self) -> List[Dict[str, Any]]:
        return self._request("/api/v1/findings")

    # --- HITL Approval Gate ---
    def request_hitl(
        self, agent_id: str, tool_name: str, target: str, command_preview: str, risk_tier: str
    ) -> Dict[str, Any]:
        payload = {
            "agent_id": agent_id,
            "tool_name": tool_name,
            "target": target,
            "command_preview": command_preview,
            "risk_tier": risk_tier,
        }
        return self._request("/api/v1/hitl/pending", method="POST", payload=payload)

    def get_hitl_request(self, hitl_id: str) -> Dict[str, Any]:
        return self._request(f"/api/v1/hitl/{hitl_id}")

    def list_pending_hitl(self) -> List[Dict[str, Any]]:
        return self._request("/api/v1/hitl/pending")

    def approve_hitl(self, hitl_id: str) -> Dict[str, Any]:
        return self._request(f"/api/v1/hitl/{hitl_id}/approve", method="POST")

    def deny_hitl(self, hitl_id: str) -> Dict[str, Any]:
        return self._request(f"/api/v1/hitl/{hitl_id}/deny", method="POST")

    # --- Ephemeral Agent Token ---
    def create_agent_token(
        self,
        agent_id: str,
        role: str,
        target_scope: str,
        allowed_tools: Optional[List[str]] = None,
        valid_seconds: int = 3600,
    ) -> Dict[str, Any]:
        payload = {
            "agent_id": agent_id,
            "role": role,
            "target_scope": target_scope,
            "allowed_tools": allowed_tools or ["*"],
            "valid_seconds": valid_seconds,
        }
        return self._request("/api/v1/auth/agent-token", method="POST", payload=payload)

    # --- Persistent Blackboard ---
    def record_service(
        self, target: str, port: int, service_name: str, banner: str, discovered_by: str = "agent-recon"
    ) -> Dict[str, Any]:
        payload = {
            "target": target,
            "port": port,
            "service_name": service_name,
            "banner": banner,
            "discovered_by": discovered_by,
            "timestamp": datetime.now(timezone.utc).isoformat(),
        }
        return self._request("/api/v1/blackboard/services", method="POST", payload=payload)

    def list_services(self, target: Optional[str] = None) -> List[Dict[str, Any]]:
        endpoint = f"/api/v1/blackboard/services?target={target}" if target else "/api/v1/blackboard/services"
        return self._request(endpoint)

    def record_endpoint(
        self, url: str, method: str, status_code: int, discovered_by: str = "agent-dast"
    ) -> Dict[str, Any]:
        payload = {
            "url": url,
            "method": method,
            "status_code": status_code,
            "discovered_by": discovered_by,
            "timestamp": datetime.now(timezone.utc).isoformat(),
        }
        return self._request("/api/v1/blackboard/endpoints", method="POST", payload=payload)

    def list_endpoints(self) -> List[Dict[str, Any]]:
        return self._request("/api/v1/blackboard/endpoints")

    def record_candidate(
        self,
        candidate_id: str,
        title: str,
        target: str,
        severity: str,
        evidence: str,
        source_agent: str,
        status: str = "open",
    ) -> Dict[str, Any]:
        payload = {
            "id": candidate_id,
            "title": title,
            "target": target,
            "severity": severity,
            "evidence": evidence,
            "source_agent": source_agent,
            "status": status,
            "timestamp": datetime.now(timezone.utc).isoformat(),
        }
        return self._request("/api/v1/blackboard/candidates", method="POST", payload=payload)

    def list_candidates(self) -> List[Dict[str, Any]]:
        return self._request("/api/v1/blackboard/candidates")

    def record_confirmed(
        self,
        confirmed_id: str,
        title: str,
        target: str,
        severity: str,
        cvss_score: Optional[float],
        confirmed_by: str,
    ) -> Dict[str, Any]:
        payload = {
            "id": confirmed_id,
            "title": title,
            "target": target,
            "severity": severity,
            "cvss_score": cvss_score,
            "confirmed_by": confirmed_by,
            "timestamp": datetime.now(timezone.utc).isoformat(),
        }
        return self._request("/api/v1/blackboard/confirmed", method="POST", payload=payload)

    def list_confirmed(self) -> List[Dict[str, Any]]:
        return self._request("/api/v1/blackboard/confirmed")

    def get_blackboard_summary(self, target: Optional[str] = None) -> Dict[str, Any]:
        endpoint = "/api/v1/blackboard/summary"
        if target:
            endpoint += f"?target={urllib.parse.quote(target)}"
        return self._request(endpoint)

    # --- Audit Trail ---
    def list_audit_logs(self, limit: int = 50) -> List[Dict[str, Any]]:
        return self._request(f"/api/v1/audit/logs?limit={limit}")
