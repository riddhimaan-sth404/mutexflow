"""Disabled standalone autonomous swarm entry point for the MutexFlow bundle.

The managed integration intentionally exposes only passive, local workspace
analysis. No environment variable can enable the vendored autonomous swarm or
its network/shell tooling. A future reintroduction requires a separately
reviewed sandbox, target authorization, Citadel approval, and audit design.
"""


def main():
    raise SystemExit(
        "The standalone Orchestral swarm runner is disabled in the MutexFlow integration. "
        "Use the managed passive local-analysis workflow instead."
    )


if __name__ == "__main__":
    main()
