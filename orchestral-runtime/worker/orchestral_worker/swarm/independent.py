import concurrent.futures
from typing import List, Dict, Any, Optional
from orchestral_worker.client import OrchestralClient
from orchestral_worker.guard.scope import ScopeGuard
from orchestral_worker.guard.hitl import HITLInterceptor, RiskTier
from orchestral_worker.agent_runner import AgentRunner
from orchestral_worker.tools.registry import ToolRegistry


class IndependentWorkerPool:
    """
    Manages isolated, decoupled autonomous workers.
    Each worker operates independently without inter-agent messaging,
    executing batch tasks (e.g., SAST repository audits, parallel port scans).

    Workers run real tools through AgentRunner; they never fabricate findings.
    """

    def __init__(
        self,
        client: OrchestralClient,
        scope_guard: ScopeGuard,
        tool_registry: Optional[ToolRegistry] = None,
        hitl: Optional[HITLInterceptor] = None,
    ):
        self.client = client
        self.scope_guard = scope_guard
        self.hitl = hitl or HITLInterceptor(auto_approve_tier=RiskTier.PASSIVE, client=client)
        self.tools = tool_registry or ToolRegistry(scope_guard=scope_guard, hitl=self.hitl)

    def _execute_worker_task(self, worker_id: str, target: str, task_type: str) -> Dict[str, Any]:
        # Validate scope before doing any work.
        self.scope_guard.validate_target(target)

        runner = AgentRunner(
            agent_id=worker_id,
            role=task_type,
            target=target,
            scope=self.scope_guard.allowlist,
            client=self.client,
            tool_registry=self.tools,
            mode="independent",
            hitl_interceptor=self.hitl,
        )
        return runner.run()

    def run_batch_tasks(
        self,
        targets: List[str],
        task_type: str = "sast",
        max_concurrency: int = 5,
    ) -> List[Dict[str, Any]]:
        print(f"\n[INDEPENDENT POOL] Spawning {len(targets)} isolated workers for task '{task_type}'")
        results: List[Dict[str, Any]] = []
        if not targets:
            print("[INDEPENDENT POOL] No targets supplied; nothing to do.")
            return results

        workers = max(1, min(len(targets), max_concurrency))
        with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as executor:
            future_to_target = {
                executor.submit(self._execute_worker_task, f"worker-{i+1}", target, task_type): target
                for i, target in enumerate(targets)
            }
            for future in concurrent.futures.as_completed(future_to_target):
                t = future_to_target[future]
                try:
                    res = future.result()
                    results.append(res)
                    print(f"  [OK] Operative completed task on {t} ({res.get('findings_count', 0)} findings)")
                except Exception as e:
                    print(f"  [ERROR] Operative failed on {t}: {e}")
                    results.append({
                        "agent_id": "unknown",
                        "target": t,
                        "status": "failed",
                        "error": str(e),
                        "findings": [],
                        "findings_count": 0,
                    })
        kali = self.tools.get_tool("kali_terminal")
        if kali and hasattr(kali, "stop_all"):
            try:
                kali.stop_all()
            except Exception:
                pass
        return results
