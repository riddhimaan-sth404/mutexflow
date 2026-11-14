import threading
import uuid
from typing import Dict, List, Any, Optional
from datetime import datetime, timezone


class Blackboard:
    """
    Centralized Collaborative Blackboard for Coordinated Swarms.
    Allows specialized agents (Recon, SAST, DAST, Triage) to publish
    intermediate observations, cross-validate hypotheses, and claim tasks.

    Backed by the persistent Orchestral Citadel SQLite REST API when a client is attached,
    enabling true multi-process agent coordination.
    """

    def __init__(self, client: Optional[Any] = None):
        self._lock = threading.RLock()
        self.client = client
        self.scope_targets: List[str] = []
        self.discovered_services: List[Dict[str, Any]] = []
        self.discovered_endpoints: List[Dict[str, Any]] = []
        self.candidate_vulnerabilities: List[Dict[str, Any]] = []
        self.verified_findings: List[Dict[str, Any]] = []
        self.agent_assignments: Dict[str, Dict[str, Any]] = {}
        self.event_log: List[Dict[str, Any]] = []

    def set_scope(self, targets: List[str]):
        with self._lock:
            self.scope_targets = targets
            self._log_event("scope_defined", {"targets": targets})

    def record_service(
        self, host: str, port: int, service_name: str, banner: Optional[str] = None, discovered_by: str = "agent-recon"
    ):
        with self._lock:
            entry = {
                "host": host,
                "port": port,
                "service": service_name,
                "banner": banner or "",
                "discovered_by": discovered_by,
                "discovered_at": datetime.now(timezone.utc).isoformat(),
            }
            if not any(s.get("host") == host and s.get("port") == port for s in self.discovered_services):
                self.discovered_services.append(entry)
                self._log_event("service_discovered", entry)

            if self.client:
                try:
                    self.client.record_service(
                        target=host,
                        port=port,
                        service_name=service_name,
                        banner=banner or "",
                        discovered_by=discovered_by,
                    )
                except Exception as e:
                    print(f"[Blackboard Warning] Failed to sync service to Core: {e}")

    def record_endpoint(
        self, url: str, method: str = "GET", status_code: int = 200, discovered_by: str = "agent-dast"
    ):
        with self._lock:
            entry = {
                "url": url,
                "method": method,
                "status_code": status_code,
                "discovered_by": discovered_by,
                "discovered_at": datetime.now(timezone.utc).isoformat(),
            }
            if not any(e.get("url") == url and e.get("method") == method for e in self.discovered_endpoints):
                self.discovered_endpoints.append(entry)
                self._log_event("endpoint_discovered", entry)

            if self.client:
                try:
                    self.client.record_endpoint(
                        url=url,
                        method=method,
                        status_code=status_code,
                        discovered_by=discovered_by,
                    )
                except Exception as e:
                    print(f"[Blackboard Warning] Failed to sync endpoint to Core: {e}")

    def record_candidate_vuln(
        self,
        agent_id: str,
        title: str,
        target: str,
        severity: str,
        details: Any,
        candidate_id: Optional[str] = None,
    ):
        cid = candidate_id or str(uuid.uuid4())
        evidence_str = str(details)
        with self._lock:
            entry = {
                "id": cid,
                "reported_by": agent_id,
                "title": title,
                "target": target,
                "severity": severity,
                "details": details,
                "status": "candidate",
                "reported_at": datetime.now(timezone.utc).isoformat(),
            }
            # Deduplicate within this engagement by (title, target).
            if any(
                c.get("title") == title and c.get("target") == target
                for c in self.candidate_vulnerabilities
            ):
                return
            self.candidate_vulnerabilities.append(entry)
            self._log_event("candidate_vuln_recorded", entry)

            if self.client:
                try:
                    self.client.record_candidate(
                        candidate_id=cid,
                        title=title,
                        target=target,
                        severity=severity,
                        evidence=evidence_str,
                        source_agent=agent_id,
                        status="candidate",
                    )
                except Exception as e:
                    print(f"[Blackboard Warning] Failed to sync candidate to Core: {e}")

    def confirm_finding(
        self,
        agent_id: str,
        title: str,
        target: str,
        severity: str,
        cvss_score: Optional[float] = None,
        confirmed_id: Optional[str] = None,
    ):
        fid = confirmed_id or str(uuid.uuid4())
        with self._lock:
            entry = {
                "id": fid,
                "title": title,
                "target": target,
                "severity": severity,
                "cvss_score": cvss_score,
                "confirmed_by": agent_id,
                "confirmed_at": datetime.now(timezone.utc).isoformat(),
            }
            if any(
                f.get("title") == title and f.get("target") == target
                for f in self.verified_findings
            ):
                return
            if self.client:
                try:
                    remote = self.client.list_confirmed()
                    if any(
                        f.get("title") == title and f.get("target") == target
                        for f in remote
                    ):
                        return
                except Exception:
                    pass
            self.verified_findings.append(entry)
            self._log_event("finding_verified", entry)

            if self.client:
                try:
                    self.client.record_confirmed(
                        confirmed_id=fid,
                        title=title,
                        target=target,
                        severity=severity,
                        cvss_score=cvss_score,
                        confirmed_by=agent_id,
                    )
                except Exception as e:
                    print(f"[Blackboard Warning] Failed to sync confirmed finding to Core: {e}")

    def get_services(self, target: Optional[str] = None) -> List[Dict[str, Any]]:
        if self.client:
            try:
                remote = self.client.list_services(target=target)
                if remote:
                    return remote
            except Exception:
                pass
        with self._lock:
            services = list(self.discovered_services)
        return [s for s in services if not target or s.get("host") == target or s.get("target") == target]

    def get_endpoints(self, target: Optional[str] = None) -> List[Dict[str, Any]]:
        if self.client:
            try:
                remote = self.client.list_endpoints()
                if remote:
                    if not target:
                        return remote
                    return [e for e in remote if target in str(e.get("url", ""))]
            except Exception:
                pass
        with self._lock:
            endpoints = list(self.discovered_endpoints)
        if not target:
            return endpoints
        return [e for e in endpoints if target in str(e.get("url", ""))]

    def get_candidates(self, target: Optional[str] = None) -> List[Dict[str, Any]]:
        if self.client:
            try:
                remote = self.client.list_candidates()
                if remote:
                    if not target:
                        return remote
                    return [c for c in remote if c.get("target") == target]
            except Exception:
                pass
        with self._lock:
            candidates = list(self.candidate_vulnerabilities)
        if not target:
            return candidates
        return [c for c in candidates if c.get("target") == target]

    def get_confirmed(self, target: Optional[str] = None) -> List[Dict[str, Any]]:
        if self.client:
            try:
                remote = self.client.list_confirmed()
                if remote:
                    if not target:
                        return remote
                    return [f for f in remote if f.get("target") == target]
            except Exception:
                pass
        with self._lock:
            confirmed = list(self.verified_findings)
        if not target:
            return confirmed
        return [f for f in confirmed if f.get("target") == target]

    def assign_task(self, agent_id: str, task_name: str, task_payload: Dict[str, Any]):
        with self._lock:
            self.agent_assignments[agent_id] = {
                "task": task_name,
                "payload": task_payload,
                "assigned_at": datetime.now(timezone.utc).isoformat(),
                "status": "in_progress",
            }
            self._log_event("task_assigned", {"agent_id": agent_id, "task": task_name})

    def complete_task(self, agent_id: str, result: Any):
        with self._lock:
            if agent_id in self.agent_assignments:
                self.agent_assignments[agent_id]["status"] = "completed"
                self.agent_assignments[agent_id]["result"] = result
            self._log_event("task_completed", {"agent_id": agent_id})

    def _log_event(self, event_type: str, data: Any):
        self.event_log.append({
            "timestamp": datetime.now(timezone.utc).isoformat(),
            "event": event_type,
            "data": data,
        })

    def snapshot(self) -> Dict[str, Any]:
        if self.client:
            try:
                target = self.scope_targets[0] if len(self.scope_targets) == 1 else None
                summary = self.client.get_blackboard_summary(target=target)
                return {
                    "scope_targets": list(self.scope_targets),
                    "services_count": summary.get("total_services", 0),
                    "endpoints_count": summary.get("total_endpoints", 0),
                    "candidates_count": summary.get("candidates", 0),
                    "verified_findings_count": summary.get("confirmed_vulnerabilities", 0),
                    "active_assignments": dict(self.agent_assignments),
                    "services": summary.get("services", []),
                    "endpoints": summary.get("endpoints", []),
                    "candidates": summary.get("candidates_list", []),
                    "confirmed": summary.get("confirmed_list", []),
                }
            except Exception:
                pass

        with self._lock:
            return {
                "scope_targets": list(self.scope_targets),
                "services_count": len(self.discovered_services),
                "endpoints_count": len(self.discovered_endpoints),
                "candidates_count": len(self.candidate_vulnerabilities),
                "verified_findings_count": len(self.verified_findings),
                "active_assignments": dict(self.agent_assignments),
                "services": list(self.discovered_services),
                "endpoints": list(self.discovered_endpoints),
                "candidates": list(self.candidate_vulnerabilities),
                "confirmed": list(self.verified_findings),
            }
