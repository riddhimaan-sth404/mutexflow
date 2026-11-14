import threading
import time
from typing import Dict, Any, List, Optional
from concurrent.futures import ThreadPoolExecutor, Future, wait

from orchestral_worker.client import OrchestralClient
from orchestral_worker.agent_runner import AgentRunner
from orchestral_worker.tools.registry import ToolRegistry
from orchestral_worker.guard.hitl import HITLInterceptor, RiskTier


class ManagedAgent:
    def __init__(self, agent_id: str, role: str, target: str, runner: Optional[AgentRunner] = None):
        self.agent_id = agent_id
        self.role = role
        self.target = target
        self.runner = runner
        self.start_time = time.time()
        self.future: Optional[Future] = None
        self.is_alive = True
        self.result: Optional[Dict[str, Any]] = None
        self.error: Optional[str] = None
        self.cancel_event = threading.Event()


class AgentSupervisor:
    """
    Lifecycle Supervisor for Orchestral Agents.
    Manages spawning, token budgets, execution timeouts, process monitoring,
    and graceful teardown for multi-agent security engagements.
    """

    def __init__(
        self,
        client: OrchestralClient,
        max_workers: int = 8,
        token_budget_per_agent: int = 50000,
        timeout_seconds: int = 300,
        monitor_interval: float = 2.0,
        auto_approve_tier: RiskTier = RiskTier.ACTIVE,
    ):
        self.client = client
        self.max_workers = max_workers
        self.token_budget_per_agent = token_budget_per_agent
        self.timeout_seconds = timeout_seconds
        self.monitor_interval = monitor_interval
        self.auto_approve_tier = auto_approve_tier
        self.executor = ThreadPoolExecutor(max_workers=max_workers)
        self.active_agents: Dict[str, ManagedAgent] = {}
        self._lock = threading.Lock()
        self._stop_monitor = threading.Event()
        self._monitor_thread: Optional[threading.Thread] = None

    def spawn_agent(
        self,
        agent_id: str,
        role: str,
        target: str,
        scope: List[str],
        tool_registry: Optional[ToolRegistry] = None,
        max_iterations: int = 8,
    ) -> str:
        """
        Issues an ephemeral agent token, instantiates an AgentRunner,
        and launches it inside the supervisor pool.
        """
        with self._lock:
            # 1. Acquire ephemeral token from Core
            try:
                tok_res = self.client.create_agent_token(
                    agent_id=agent_id,
                    role=role,
                    target_scope=target,
                    allowed_tools=["*"],
                    valid_seconds=self.timeout_seconds + 60,
                )
                agent_token = tok_res.get("token")
            except Exception as e:
                print(f"[Supervisor] Could not acquire agent token ({e}); proceeding with main API key.")
                agent_token = self.client.api_key

            # 2. Scoped client for agent
            agent_client = OrchestralClient(base_url=self.client.base_url, api_key=agent_token)
            agent_hitl = HITLInterceptor(
                auto_approve_tier=self.auto_approve_tier,
                client=agent_client,
            )

            managed = ManagedAgent(agent_id=agent_id, role=role, target=target, runner=None)  # type: ignore[arg-type]
            managed.cancel_event = threading.Event()

            # 3. Create runner with cancellation and budget wired in.
            runner = AgentRunner(
                agent_id=agent_id,
                role=role,
                target=target,
                scope=scope,
                client=agent_client,
                tool_registry=tool_registry,
                max_iterations=max_iterations,
                mode="supervised",
                token_budget=self.token_budget_per_agent,
                cancel_event=managed.cancel_event,
                hitl_interceptor=agent_hitl,
            )
            managed.runner = runner
            managed.future = self.executor.submit(self._run_supervised, managed)
            self.active_agents[agent_id] = managed

            print(f"[Supervisor] Spawned agent '{agent_id}' (role: {role}) on target '{target}'.")
            return agent_id

    def _run_supervised(self, managed: ManagedAgent):
        try:
            res = managed.runner.run()
            managed.result = res
            if res.get("status") == "failed":
                managed.error = res.get("error")
        except Exception as e:
            managed.error = str(e)
            print(f"[Supervisor] Agent '{managed.agent_id}' crashed: {e}")
        finally:
            managed.is_alive = False

    def kill_agent(self, agent_id: str) -> bool:
        """Requests cooperative cancellation of a running agent."""
        with self._lock:
            managed = self.active_agents.get(agent_id)
            if not managed:
                return False
            managed.cancel_event.set()
            managed.is_alive = False
            if managed.future and not managed.future.done():
                managed.future.cancel()
            self.client.register_or_update_agent({
                "id": agent_id,
                "name": agent_id,
                "role": managed.role,
                "mode": "supervised",
                "status": "cancelled",
                "current_task": "KILLED_BY_SUPERVISOR",
                "tokens_used": managed.runner.tokens_used,
                "cost_usd": managed.runner.estimated_cost,
                "last_seen": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            })
            print(f"[Supervisor] Agent '{agent_id}' cancelled.")
            return True

    def monitor_and_enforce_limits(self):
        """Checks runtime and token limits across all active agents."""
        with self._lock:
            now = time.time()
            for agent_id, managed in list(self.active_agents.items()):
                if not managed.is_alive:
                    continue

                elapsed = now - managed.start_time
                if elapsed > self.timeout_seconds:
                    print(f"[Supervisor ALERT] Agent '{agent_id}' exceeded timeout ({elapsed:.1f}s > {self.timeout_seconds}s). Killing.")
                    self.kill_agent(agent_id)
                    continue

                if managed.runner.tokens_used > self.token_budget_per_agent:
                    print(f"[Supervisor ALERT] Agent '{agent_id}' exceeded token budget ({managed.runner.tokens_used} > {self.token_budget_per_agent}). Killing.")
                    self.kill_agent(agent_id)

    def _monitor_loop(self):
        while not self._stop_monitor.wait(self.monitor_interval):
            self.monitor_and_enforce_limits()

    def start_monitor(self):
        if self._monitor_thread and self._monitor_thread.is_alive():
            return
        self._stop_monitor.clear()
        self._monitor_thread = threading.Thread(target=self._monitor_loop, daemon=True)
        self._monitor_thread.start()

    def stop_monitor(self):
        self._stop_monitor.set()
        if self._monitor_thread:
            self._monitor_thread.join(timeout=self.monitor_interval + 1)

    def wait_all(self, timeout: Optional[float] = None) -> List[Dict[str, Any]]:
        """Waits for all active agents while enforcing timeout and budget limits."""
        deadline = time.time() + timeout if timeout is not None else None
        self.start_monitor()
        try:
            pending = {m.future for m in self.active_agents.values() if m.future}
            while pending:
                slice_timeout = self.monitor_interval
                if deadline is not None:
                    remaining = deadline - time.time()
                    if remaining <= 0:
                        break
                    slice_timeout = min(slice_timeout, remaining)
                _, pending = wait(pending, timeout=slice_timeout)
                self.monitor_and_enforce_limits()
        finally:
            self.stop_monitor()

        results = []
        for agent_id, managed in list(self.active_agents.items()):
            results.append({
                "agent_id": agent_id,
                "role": managed.role,
                "target": managed.target,
                "status": (managed.result or {}).get("status", "failed" if managed.error else "unknown"),
                "result": managed.result,
                "error": managed.error,
                "tokens_used": managed.runner.tokens_used,
                "cost_usd": managed.runner.estimated_cost,
            })
        return results

    def list_active(self) -> List[Dict[str, Any]]:
        with self._lock:
            return [
                {
                    "agent_id": m.agent_id,
                    "role": m.role,
                    "target": m.target,
                    "elapsed_sec": round(time.time() - m.start_time, 1),
                    "is_alive": m.is_alive,
                    "tokens_used": m.runner.tokens_used,
                }
                for m in self.active_agents.values()
            ]

    def shutdown(self):
        """Cancel all agents, stop sandboxes, and release the executor."""
        for agent_id in list(self.active_agents):
            self.kill_agent(agent_id)
        self.stop_monitor()
        # Stop any sandboxes owned by the shared registry.
        try:
            kali = self.active_agents and next(iter(self.active_agents.values())).runner.tools.get_tool("kali_terminal")
            if kali and hasattr(kali, "stop_all"):
                kali.stop_all()
        except Exception:
            pass
        self.executor.shutdown(wait=False, cancel_futures=True)
