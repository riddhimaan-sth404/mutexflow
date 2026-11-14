"""Managed worker tool catalog.

Only the passive local secret-pattern scanner is available in MutexFlow.
Network probes, shell execution, and other dormant upstream tools are never
imported or registered by this managed worker. There is no environment
variable or runtime opt-in that enables them.
"""
from typing import Dict, Any, List, Optional

from orchestral_worker.guard.scope import ScopeGuard
from orchestral_worker.guard.hitl import HITLInterceptor
from orchestral_worker.tools.base import BaseSecurityTool
from orchestral_worker.tools.secret_scanner import NativeSecretScannerTool


def extended_tools_enabled() -> bool:
    """Compatibility shim: extended tools are hard-disabled in MutexFlow."""
    return False


class ToolRegistry:
    """Managed catalog containing only passive local workspace analysis."""

    def __init__(self, scope_guard: Optional[ScopeGuard] = None, hitl: Optional[HITLInterceptor] = None):
        self.scope_guard = scope_guard
        self.hitl = hitl
        self._tools: Dict[str, BaseSecurityTool] = {}
        self._register_default_tools()

    def _register_default_tools(self):
        self.register(NativeSecretScannerTool(scope_guard=self.scope_guard, hitl_interceptor=self.hitl))

    def register(self, tool: BaseSecurityTool):
        self._tools[tool.name] = tool

    def get_tool(self, name: str) -> Optional[BaseSecurityTool]:
        return self._tools.get(name)

    def list_tools(self) -> List[Dict[str, Any]]:
        seen = set()
        out = []
        for tool in self._tools.values():
            if tool.name not in seen:
                seen.add(tool.name)
                out.append({
                    "name": tool.name,
                    "description": tool.description,
                    "risk_tier": tool.risk_tier.value,
                })
        return out

    def get_tools_for_role(self, role: str) -> List[str]:
        # Role profiles may still name dormant upstream tools. Only advertise
        # tools that are actually registered in this managed worker.
        role_lower = (role or "").lower()
        if "sast" in role_lower or "code" in role_lower or "athena" in role_lower:
            requested = ["native_secret_scanner", "codeql_analyzer"]
        elif "triage" in role_lower or "justiciar" in role_lower:
            requested = ["native_secret_scanner", "codeql_analyzer"]
        elif "recon" in role_lower or "osint" in role_lower or "sentinel" in role_lower:
            requested = ["native_secret_scanner", "native_port_scanner", "native_http_probe"]
        elif "dast" in role_lower or "web" in role_lower or "cerberus" in role_lower:
            requested = ["native_secret_scanner", "native_http_probe", "native_port_scanner"]
        elif "coord" in role_lower or "archon" in role_lower:
            requested = list(self._tools.keys())
        else:
            requested = ["native_secret_scanner"]
        return [name for name in requested if name in self._tools]
