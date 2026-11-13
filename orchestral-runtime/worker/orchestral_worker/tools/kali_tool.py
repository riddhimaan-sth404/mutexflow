import threading
from typing import Dict, Any, Optional
from orchestral_worker.tools.base import BaseSecurityTool
from orchestral_worker.guard.hitl import RiskTier
from orchestral_worker.sandbox.kali import KaliSandboxManager


class KaliTerminalTool(BaseSecurityTool):
    """
    Exposes an exclusive, sandboxed Kali Linux terminal environment to the agent.
    Tools (Nmap, John the Ripper, Nikto, Trufflehog, Semgrep, curl, etc.) execute inside
    an ephemeral rootless container dedicated to the calling agent.
    """

    def __init__(self, **kwargs):
        super().__init__(
            name="kali_terminal",
            description="Executes terminal security tools (Nmap, John the Ripper, Nikto, curl) inside an agent-exclusive Kali sandbox",
            risk_tier=RiskTier.PASSIVE,  # Dynamically evaluated per command in sandbox manager
            **kwargs,
        )
        self._sandboxes: Dict[str, KaliSandboxManager] = {}
        self._lock = threading.Lock()

    def risk_tier_for_execution(self, target: str, **kwargs) -> RiskTier:
        return KaliSandboxManager.classify_risk_tier(kwargs.get("command", ""))

    def _get_or_create_sandbox(self, agent_id: str) -> KaliSandboxManager:
        with self._lock:
            if agent_id not in self._sandboxes:
                mgr = KaliSandboxManager(
                    agent_id=agent_id,
                    scope_guard=self.scope_guard,
                    hitl_interceptor=self.hitl,
                )
                mgr.start_sandbox()
                self._sandboxes[agent_id] = mgr
            return self._sandboxes[agent_id]

    def _run(self, target: str, command: str = "uname -a", agent_id: str = "agent-kali", **kwargs) -> Dict[str, Any]:
        sandbox = self._get_or_create_sandbox(agent_id)
        if not sandbox._is_running:
            return {
                "tool": self.name,
                "target": target,
                "status": "sandbox_unavailable",
                "error": (
                    "Kali sandbox could not be started (container runtime or image "
                    "unavailable). Refusing to simulate execution."
                ),
                "command": command,
            }
        return sandbox.execute(command=command, target=target)

    def stop_all(self) -> None:
        with self._lock:
            for sandbox in self._sandboxes.values():
                sandbox.stop_sandbox()
            self._sandboxes.clear()


# Strategic Alias
TartarusTerminalTool = KaliTerminalTool
