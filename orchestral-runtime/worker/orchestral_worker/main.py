import os
import sys
import argparse
from orchestral_worker.client import OrchestralClient
from orchestral_worker.guard.scope import ScopeGuard, ScopeViolationError
from orchestral_worker.guard.hitl import HITLInterceptor, RiskTier
from orchestral_worker.swarm.coordinator import CoordinatedSwarmCoordinator
from orchestral_worker.swarm.independent import IndependentWorkerPool


def main():
    parser = argparse.ArgumentParser(
        description="Orchestral Python Security Agent Worker & Swarm Runner"
    )
    parser.add_argument(
        "--core-url",
        default=os.environ.get("ORCHESTRAL_CORE_URL", "http://127.0.0.1:8088"),
        help="Orchestral Citadel API base URL",
    )
    parser.add_argument(
        "--api-key",
        default=os.environ.get("ORCHESTRAL_API_KEY", ""),
        help="API Key for Orchestral Citadel authentication",
    )
    parser.add_argument(
        "--mode",
        choices=["coordinated", "independent"],
        default="coordinated",
        help="Execution mode: coordinated multi-agent swarm vs independent worker pool",
    )
    parser.add_argument(
        "--targets",
        default="example.internal.local",
        help="Target domain(s) or CIDR(s), comma-separated",
    )
    parser.add_argument(
        "--allow-scope",
        default="*.internal.local,example.internal.local,10.0.0.0/8",
        help="Authorized domains and CIDRs, comma-separated",
    )

    args = parser.parse_args()

    if not args.api_key:
        print("[!] Error: API Key is required. Provide via --api-key or ORCHESTRAL_API_KEY env var.")
        sys.exit(1)

    print("=======================================================")
    print("[ORCHESTRAL] Security Research Swarm Engine")
    print(f"  Citadel Gateway: {args.core_url}")
    print(f"  Execution Mode:  {args.mode.upper()}")
    print("=======================================================")

    # Initialize client & test connectivity
    client = OrchestralClient(base_url=args.core_url, api_key=args.api_key)
    try:
        health = client.health()
        print(f"[OK] Connected to Orchestral Citadel (Status: {health.get('status')})")
    except Exception as e:
        print(f"[ERROR] Failed to connect to Orchestral Citadel: {e}")
        sys.exit(1)

    # Initialize ScopeGuard. The allowlist parser handles bare IPs, CIDRs,
    # wildcard domains, and IPv6 literals.
    allowlist = [s.strip() for s in args.allow_scope.split(",") if s.strip()]
    scope_guard = ScopeGuard(allowlist=allowlist)

    # Initialize HITL with a Core client so active actions can be approved
    # from the TUI/CLI instead of falling back to a blocking stdin prompt.
    hitl = HITLInterceptor(auto_approve_tier=RiskTier.PASSIVE, client=client)

    target_list = [t.strip() for t in args.targets.split(",")]

    if args.mode == "coordinated":
        coordinator = CoordinatedSwarmCoordinator(
            client=client,
            scope_guard=scope_guard,
            hitl_interceptor=hitl,
        )
        for t in target_list:
            coordinator.run_engagement(
                target=t,
                research_objective=f"Autonomous attack surface analysis and CVE verification on {t}",
            )
    else:
        pool = IndependentWorkerPool(client=client, scope_guard=scope_guard)
        pool.run_batch_tasks(target_list, task_type="sast")


if __name__ == "__main__":
    main()
