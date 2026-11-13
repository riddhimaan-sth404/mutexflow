from typing import Dict, Any, List, Optional
from orchestral_worker.guard.scope import ScopeGuard
from orchestral_worker.guard.hitl import HITLInterceptor
from orchestral_worker.tools.base import BaseSecurityTool
from orchestral_worker.tools.secret_scanner import NativeSecretScannerTool
from orchestral_worker.tools.kali_tool import KaliTerminalTool, TartarusTerminalTool
from orchestral_worker.tools.codeql import CodeQLTool
from orchestral_worker.tools.port_scanner import NativePortScannerTool
from orchestral_worker.tools.http_probe import NativeHttpProbeTool


class ToolRegistry:
    """
    Unified Tool Catalog for Orchestral AI Agents.
    Standardized on Tartarus terminal execution (Nmap, JtR, curl, etc.)
    with local static analysis (CodeQL, SecretScanner).
    """

    def __init__(self, scope_guard: Optional[ScopeGuard] = None, hitl: Optional[HITLInterceptor] = None):
        self.scope_guard = scope_guard
        self.hitl = hitl
        self._tools: Dict[str, BaseSecurityTool] = {}
        self._register_default_tools()

    def _register_default_tools(self):
        # 1. Primary Sandboxed Terminal Tool (Tartarus Kali Execution)
        kali = KaliTerminalTool(scope_guard=self.scope_guard, hitl_interceptor=self.hitl)
        self.register(kali)
        self._tools["tartarus_terminal"] = kali
        self._tools["tartarus_exec"] = kali

        # 2. Local Code and Secret Analysis
        self.register(NativeSecretScannerTool(scope_guard=self.scope_guard, hitl_interceptor=self.hitl))
        self.register(CodeQLTool(scope_guard=self.scope_guard, hitl_interceptor=self.hitl))

        # 3. Lightweight Fallback Tools
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
            return ["native_secret_scanner", "codeql_analyzer", "kali_terminal"]
        elif "triage" in role_lower or "justiciar" in role_lower:
            return ["native_secret_scanner", "codeql_analyzer"]
        elif "recon" in role_lower or "osint" in role_lower or "sentinel" in role_lower:
            # Recon needs both network discovery and local secret/static checks.
            return ["kali_terminal", "native_port_scanner", "native_http_probe", "native_secret_scanner"]
        elif "dast" in role_lower or "web" in role_lower or "cerberus" in role_lower:
            return ["kali_terminal", "native_http_probe", "native_port_scanner"]
        elif "coord" in role_lower or "archon" in role_lower:
            return [t.name for t in self._tools.values()]
        return ["kali_terminal"]
