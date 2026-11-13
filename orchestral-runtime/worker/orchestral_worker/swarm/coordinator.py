import json
import sys
from typing import List, Dict, Any, Optional
from datetime import datetime, timezone
from orchestral_worker.client import OrchestralClient
from orchestral_worker.guard.scope import ScopeGuard
from orchestral_worker.guard.hitl import HITLInterceptor, RiskTier
from orchestral_worker.swarm.blackboard import Blackboard
from orchestral_worker.agent_runner import AgentRunner, _is_local_path
from orchestral_worker.tools.registry import ToolRegistry


_SEVERITY_CVSS = {
    "CRITICAL": 9.1,
    "HIGH": 7.5,
    "MEDIUM": 5.5,
    "LOW": 3.1,
    "INFO": 0.0,
}


class CoordinatedSwarmCoordinator:
    """
    Orchestrates collaborative, multi-agent security research engagements.
    Specialist agents publish intermediate intel to a shared persistent blackboard,
    progressing through: Recon -> Vulnerability Analysis -> HITL Approval -> Verification -> Reporting.
    """

    def __init__(
        self,
        client: OrchestralClient,
        scope_guard: ScopeGuard,
        hitl_interceptor: Optional[HITLInterceptor] = None,
        tool_registry: Optional[ToolRegistry] = None,
        **kwargs,
    ):
        self.client = client
        self.scope_guard = scope_guard
        self.hitl = hitl_interceptor or HITLInterceptor(
            auto_approve_tier=RiskTier.PASSIVE,
            client=self.client,
        )
        self.tools = tool_registry or ToolRegistry(scope_guard=self.scope_guard, hitl=self.hitl)
        self.blackboard = Blackboard(client=self.client)

    def _record_recon(self, target: str, observations: List[Dict[str, Any]], discovered_by: str = "agent-recon-01"):
        """Translate recon tool output into blackboard services/endpoints."""
        for obs in observations:
            res = obs.get("result", {})
            if not isinstance(res, dict):
                continue

            if res.get("tool") == "native_port_scanner" or res.get("open_ports"):
                for open_port in res.get("open_ports", []):
                    if isinstance(open_port, dict):
                        port_num = open_port.get("port")
                        banner = open_port.get("banner", "")
                    else:
                        port_num = int(open_port)
                        banner = ""
                    if port_num is None:
                        continue
                    self.blackboard.record_service(
                        host=target,
                        port=port_num,
                        service_name=self._service_name(port_num),
                        banner=banner or f"Port {port_num} open",
                        discovered_by=discovered_by,
                    )

            if res.get("tool") == "native_http_probe" or "status_code" in res:
                if res.get("status_code"):
                    url = res.get("target") or target
                    if not str(url).startswith("http"):
                        url = f"http://{url}"
                    self.blackboard.record_endpoint(
                        url=url,
                        method="GET",
                        status_code=res.get("status_code", 0),
                        discovered_by=discovered_by,
                    )

            # Parse nmap output from the sandbox terminal as a fallback.
            stdout = res.get("stdout") or ""
            if "nmap" in str(res.get("tool", "")) or "Nmap scan report" in stdout:
                for port in self._parse_nmap_ports(stdout):
                    self.blackboard.record_service(
                        host=target,
                        port=port,
                        service_name=self._service_name(port),
                        banner=f"Port {port} open (nmap)",
                        discovered_by=discovered_by,
                    )

    @staticmethod
    def _service_name(port: int) -> str:
        return {
            21: "ftp", 22: "ssh", 23: "telnet", 25: "smtp", 53: "dns",
            80: "http", 110: "pop3", 143: "imap", 443: "https", 3306: "mysql",
            5432: "postgres", 6379: "redis", 8000: "http-alt", 8080: "http-alt",
            8088: "http-alt", 8443: "https-alt", 9000: "http-alt",
        }.get(port, "unknown")

    @staticmethod
    def _parse_nmap_ports(stdout: str) -> List[int]:
        ports = []
        for line in (stdout or "").splitlines():
            parts = line.split()
            if len(parts) >= 2 and parts[0].isdigit() and parts[1] == "open":
                ports.append(int(parts[0]))
        return ports

    def _context(self, target: str) -> Dict[str, Any]:
        """Shared engagement context passed to every specialist."""
        return {
            "target": target,
            "services": self.blackboard.get_services(target=target),
            "endpoints": self.blackboard.get_endpoints(target=target),
            "candidates": self.blackboard.get_candidates(target=target),
            "findings": self.blackboard.get_confirmed(target=target),
        }

    def _record_candidates(self, agent_id: str, target: str, findings: List[Dict[str, Any]]):
        for f in findings:
            self.blackboard.record_candidate_vuln(
                agent_id=agent_id,
                title=f.get("title", "Discovered Vulnerability"),
                target=f.get("target", target),
                severity=f.get("severity", "MEDIUM"),
                details=f,
            )

    def _verify_candidate(self, candidate: Dict[str, Any]) -> Optional[Dict[str, Any]]:
        """Evidence-based triage. Rejects candidates with no verifiable evidence."""
        details = candidate.get("details")
        if isinstance(details, str):
            try:
                details = json.loads(details)
            except Exception:
                details = {"evidence": details}
        if not isinstance(details, dict):
            details = {}

        evidence = details.get("evidence") or candidate.get("evidence")
        if not evidence:
            return None
        evidence_str = str(evidence).strip()
        if not evidence_str or evidence_str in ("{}", "None", "null"):
            return None

        severity = str(candidate.get("severity") or details.get("severity") or "MEDIUM").upper()
        if severity not in _SEVERITY_CVSS:
            severity = "MEDIUM"

        cvss = details.get("cvss")
        if cvss is None:
            cvss = _SEVERITY_CVSS[severity]

        return {
            "title": candidate.get("title") or details.get("title") or "Confirmed Finding",
            "target": candidate.get("target") or details.get("target"),
            "severity": severity,
            "cvss_score": float(cvss),
            "evidence": evidence_str[:4000],
            "details": details,
        }

    def run_engagement(self, target: str, research_objective: str = "Assess the target's security posture") -> Dict[str, Any]:
        print(f"\n[SWARM LEAD] Initializing Coordinated Swarm for Target: {target}")
        print(f"[SWARM LEAD] Objective: {research_objective}")

        # 1. Deterministic Scope Enforcement
        self.scope_guard.validate_target(target)
        self.blackboard.set_scope([target])

        # 2. Register Lead Coordinator Agent
        self.client.register_or_update_agent({
            "id": "coord-lead",
            "name": "Orchestral Master Coordinator",
            "role": "Coordinator",
            "mode": "coordinated",
            "status": "thinking",
            "current_task": f"Decomposing attack surface for {target}",
            "tokens_used": 0,
            "cost_usd": 0.0,
            "last_seen": datetime.now(timezone.utc).isoformat(),
        })

        # --- Phase 1: Reconnaissance Specialist Execution ---
        print("\n[PHASE 1] Launching Sentinel Recon Specialist...")
        self.blackboard.assign_task("agent-recon-01", "recon", {"target": target})
        recon_runner = AgentRunner(
            agent_id="agent-recon-01",
            role="recon",
            target=target,
            scope=self.scope_guard.allowlist,
            client=self.client,
            tool_registry=self.tools,
            shared_context=self._context(target),
            mode="coordinated",
            hitl_interceptor=self.hitl,
        )
        recon_result = recon_runner.run()
        self._record_recon(target, recon_result.get("observations", []))
        self.blackboard.complete_task("agent-recon-01", {"findings": recon_result.get("findings_count", 0)})

        # --- Phase 2: DAST / Web Vuln Analysis Execution ---
        print("\n[PHASE 2] Launching Cerberus DAST Specialist...")
        self.blackboard.assign_task("agent-dast-01", "dast", {"target": target})
        dast_runner = AgentRunner(
            agent_id="agent-dast-01",
            role="dast",
            target=target,
            scope=self.scope_guard.allowlist,
            client=self.client,
            tool_registry=self.tools,
            shared_context=self._context(target),
            mode="coordinated",
            hitl_interceptor=self.hitl,
        )
        dast_result = dast_runner.run()
        self._record_recon(target, dast_result.get("observations", []), discovered_by="agent-dast-01")
        self._record_candidates("agent-dast-01", target, dast_result.get("findings", []))
        self._record_candidates("agent-recon-01", target, recon_result.get("findings", []))
        self.blackboard.complete_task("agent-dast-01", {"findings": dast_result.get("findings_count", 0)})

        # --- Phase 3: Triage & Validation Specialist Execution ---
        print("\n[PHASE 3] Launching Justiciar Triage Specialist...")
        self.blackboard.assign_task("agent-triage-01", "triage", {"target": target})
        triage_runner = AgentRunner(
            agent_id="agent-triage-01",
            role="triage",
            target=target,
            scope=self.scope_guard.allowlist,
            client=self.client,
            tool_registry=self.tools,
            shared_context=self._context(target),
            mode="coordinated",
            hitl_interceptor=self.hitl,
        )
        triage_result = triage_runner.run()

        # Ingest only target-scoped candidates and verify them with evidence.
        candidates = self.blackboard.get_candidates(target=target)
        confirmed_count = 0
        rejected_count = 0
        for cand in candidates:
            verified = self._verify_candidate(cand)
            if not verified:
                rejected_count += 1
                continue
            self.blackboard.confirm_finding(
                agent_id="agent-triage-01",
                title=verified["title"],
                target=verified["target"] or target,
                severity=verified["severity"],
                cvss_score=verified["cvss_score"],
            )
            confirmed_count += 1

        self.blackboard.complete_task(
            "agent-triage-01",
            {"confirmed": confirmed_count, "rejected": rejected_count},
        )

        # --- Phase 4: Engagement Completion ---
        total_tokens = recon_runner.tokens_used + dast_runner.tokens_used + triage_runner.tokens_used
        total_cost = recon_runner.estimated_cost + dast_runner.estimated_cost + triage_runner.estimated_cost
        self.client.register_or_update_agent({
            "id": "coord-lead",
            "name": "Orchestral Master Coordinator",
            "role": "Coordinator",
            "mode": "coordinated",
            "status": "completed",
            "current_task": "Engagement report generated and findings published",
            "tokens_used": total_tokens,
            "cost_usd": total_cost,
            "last_seen": datetime.now(timezone.utc).isoformat(),
        })

        snapshot = self.blackboard.snapshot()
        report = self._render_report(target, research_objective, snapshot)
        print(f"\n========================================================")
        print(f"[SUCCESS] Coordinated engagement completed successfully!")
        print(f"[SUMMARY] Intelligence Assessment:")
        print(f"   Services Discovered: {snapshot.get('services_count')}")
        print(f"   Endpoints Probed:    {snapshot.get('endpoints_count')}")
        print(f"   Candidate Vulns:     {snapshot.get('candidates_count')}")
        print(f"   Confirmed Findings:  {snapshot.get('verified_findings_count')}")
        print(f"========================================================\n")

        # Tear down any per-agent sandbox containers created for this run.
        kali = self.tools.get_tool("kali_terminal")
        if kali and hasattr(kali, "stop_all"):
            try:
                kali.stop_all()
            except Exception as e:
                print(f"[SWARM LEAD] Sandbox cleanup warning: {e}", file=sys.stderr)

        return {
            "status": "completed",
            "target": target,
            "objective": research_objective,
            "snapshot": snapshot,
            "report": report,
            "recon": recon_result,
            "dast": dast_result,
            "triage": triage_result,
            "tokens_used": total_tokens,
            "cost_usd": total_cost,
        }

    @staticmethod
    def _render_report(target: str, objective: str, snapshot: Dict[str, Any]) -> str:
        lines = [
            f"# Security Assessment Report",
            "",
            f"**Target:** `{target}`  ",
            f"**Objective:** {objective}  ",
            f"**Generated:** {datetime.now(timezone.utc).isoformat()}",
            "",
            "## Summary",
            "",
            f"- Services discovered: {snapshot.get('services_count', 0)}",
            f"- Endpoints probed: {snapshot.get('endpoints_count', 0)}",
            f"- Candidate vulnerabilities: {snapshot.get('candidates_count', 0)}",
            f"- Confirmed findings: {snapshot.get('verified_findings_count', 0)}",
            "",
            "## Confirmed Findings",
            "",
        ]
        confirmed = snapshot.get("confirmed", []) or []
        if not confirmed:
            lines.append("No findings were confirmed by evidence-based triage.")
        else:
            for f in confirmed:
                lines.append(
                    f"- **{f.get('severity', 'UNKNOWN')}** — {f.get('title', 'Finding')} "
                    f"(CVSS {f.get('cvss_score', 'n/a')})"
                )
        lines.append("")
        return "\n".join(lines)
