"""Unified tool catalog for Orchestral agents.

MutexFlow ships this worker with only the passive, local secret-pattern
scanner enabled. The vendored offensive/dual-use tools (Kali terminal, port
scanner, HTTP probe) and the CodeQL analyzer are DORMANT: they are imported
and registered only when the operator explicitly sets
MUTEXFLOW_ENABLE_EXTENDED_TOOLS=1. See SECURITY.md at the repository root.
"""
import os
from typing import Dict, Any, List, Optional

from orchestral_worker.guard.scope import ScopeGuard
from orchestral_worker.guard.hitl import HITLInterceptor
from orchestral_worker.tools.base import BaseSecurityTool
from orchestral_worker.tools.secret_scanner import NativeSecretScannerTool

EXTENDED_TOOLS_ENV = "MUTEXFLOW_ENABLE_EXTENDED_TOOLS"
_TRUTHY = {"1", "true", "yes", "on"}


def extended_tools_enabled() -> bool:
    return os.environ.get(EXTENDED_TOOLS_ENV, "").strip().lower() in _TRUTHY


class ToolRegistry:
    """
    Unified Tool Catalog for Orchestral AI Agents.

    By default only the passive NativeSecretScannerTool is registered. The
    sandboxed Kali terminal, CodeQL analyzer, port scanner, and HTTP probe
    are dormant upstream tooling: they are registered solely when
    MUTEXFLOW_ENABLE_EXTENDED_TOOLS is explicitly enabled. The managed
    MutexFlow worker never enables them.
    """

    def __init__(self, scope_guard: Optional[ScopeGuard] = None, hitl: Optional[HITLInterceptor] = None):
        self.scope_guard = scope_guard
        self.hitl = hitl
        self._tools: Dict[str, BaseSecurityTool] = {}
        self._register_default_tools()

    def _register_default_tools(self):
        # 1. Passive local analysis (always available).
        self.register(NativeSecretScannerTool(scope_guard=self.scope_guard, hitl_interceptor=self.hitl))

        # 2. Dormant upstream tooling. Not imported, registered, or advertised
        #    unless the operator explicitly opts in via MUTEXFLOW_ENABLE_EXTENDED_TOOLS.
        if not extended_tools_enabled():
            return

        from orchestral_worker.tools.kali_tool import KaliTerminalTool, TartarusTerminalTool
        from orchestral_worker.tools.codeql import CodeQLTool
        from orchestral_worker.tools.port_scanner import NativePortScannerTool
        from orchestral_worker.tools.http_probe import NativeHttpProbeTool

        # Primary Sandboxed Terminal Tool (Tartarus Kali Execution)
        kali = KaliTerminalTool(scope_guard=self.scope_guard, hitl_interceptor=self.hitl)
        self.register(kali)
        self._tools["tartarus_terminal"] = kali
        self._tools["tartarus_exec"] = kali

        # Local static analysis and lightweight network probes
        self.register(CodeQLTool(scope_guard=self.scope_guard, hitl_interceptor=self.hitl))
        self.register(NativePortScannerTool(scope_guard=self.scope_guard, hitl_interceptor=self.hitl))
        self.register(NativeHttpProbeTool(scope_guard=self.scope_guard, hitl_interceptor=self.hitl))

    def register(self, tool: BaseSecurityTool):
        self._tools[tool.name] = tool

    def get_tool(self, name: str) -> Optional[BaseSecurityTool]:
        return self._tools.get(name)

    def list_tools(self) -> List[Dict[str, Any]]:
        seen = set()
        out = []
        for t in self._tools.values():
            if t.name not in seen:
                seen.add(t.name)
                out.append({
                    "name": t.name,
                    "description": t.description,
                    "risk_tier": t.risk_tier.value,
                })
        return out

    def get_tools_for_role(self, role: str) -> List[str]:
        role_lower = (role or "").lower()
        # Match the most specific role first: "athena_sast" contains "athena"
        # and must not fall through to the recon branch.
        if "sast" in role_lower or "code" in role_lower or "athena" in role_lower:
            requested = ["native_secret_scanner", "codeql_analyzer", "kali_terminal"]
        elif "triage" in role_lower or "justiciar" in role_lower:
            requested = ["native_secret_scanner", "codeql_analyzer"]
        elif "recon" in role_lower or "osint" in role_lower or "sentinel" in role_lower:
            # Recon needs both network discovery and local secret/static checks.
            requested = ["kali_terminal", "native_port_scanner", "native_http_probe", "native_secret_scanner"]
        elif "dast" in role_lower or "web" in role_lower or "cerberus" in role_lower:
            requested = ["kali_terminal", "native_http_probe", "native_port_scanner"]
        elif "coord" in role_lower or "archon" in role_lower:
            requested = list(self._tools.keys())
        else:
            requested = ["kali_terminal"]
        # Never advertise a tool that is not registered (dormant tooling).
        return [name for name in requested if name in self._tools]
