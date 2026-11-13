from abc import ABC, abstractmethod
from typing import Dict, Any, Optional
from orchestral_worker.guard.scope import ScopeGuard
from orchestral_worker.guard.hitl import HITLInterceptor, RiskTier


class BaseSecurityTool(ABC):
    """
    Base class for all tools provided to Orchestral security agents.
    Enforces deterministic scope boundaries and Human-in-the-Loop gating.
    """

    def __init__(
        self,
        name: str,
        description: str,
        risk_tier: RiskTier,
        scope_guard: Optional[ScopeGuard] = None,
        hitl_interceptor: Optional[HITLInterceptor] = None,
    ):
        self.name = name
        self.description = description
        self.risk_tier = risk_tier
        self.scope_guard = scope_guard
        self.hitl = hitl_interceptor

    def risk_tier_for_execution(self, target: str, **kwargs) -> RiskTier:
        """Return the risk tier for this specific invocation.

        Tools whose risk depends on arguments (for example a terminal tool)
        override this. The default is the tool's declared static tier.
        """
        return self.risk_tier

    def execute(self, agent_id: str, target: str, **kwargs) -> Dict[str, Any]:
        """
        Executes the tool with automated ScopeGuard check and HITL sign-off.
        """
        # 1. Deterministic Scope Verification
        if self.scope_guard:
            self.scope_guard.validate_target(target)

        # 2. Human-in-the-Loop Gate. The tier is computed per invocation so a
        # passive local scanner never blocks while an active probe still does.
        risk = self.risk_tier_for_execution(target, **kwargs)
        command_repr = f"{self.name} --target {target} " + " ".join(
            f"--{k} {v}" for k, v in kwargs.items() if v is not None
        )
        if self.hitl:
            approved = self.hitl.request_approval(
                agent_id=agent_id,
                tool_name=self.name,
                target=target,
                command=command_repr,
                risk_tier=risk,
            )
            if not approved:
                return {
                    "tool": self.name,
                    "target": target,
                    "status": "denied",
                    "error": f"Execution of {self.name} on {target} denied by operator/HITL gate",
                }

        # 3. Execution. Forward the agent identity so per-agent resources
        # (for example a dedicated sandbox container) are keyed correctly.
        try:
            result = self._run(target, agent_id=agent_id, **kwargs)
        except Exception as e:  # noqa: BLE001 - surfaced to the agent as a tool error
            return {
                "tool": self.name,
                "target": target,
                "status": "error",
                "error": f"{type(e).__name__}: {e}",
            }

        if isinstance(result, dict):
            result.setdefault("tool", self.name)
            result.setdefault("target", target)
        return result

    def run(self, **kwargs) -> Dict[str, Any]:
        """Convenience alias for execute() used in agent reasoning loops."""
        agent_id = kwargs.pop("agent_id", "agent-runner")
        target = kwargs.pop("target", "")
        return self.execute(agent_id=agent_id, target=target, **kwargs)

    @abstractmethod
    def _run(self, target: str, **kwargs) -> Dict[str, Any]:
        """Actual tool execution logic."""
        pass
