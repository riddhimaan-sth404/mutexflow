"""Orchestral Swarm Orchestration: Blackboard, Coordinator, and Independent Workers."""
from .blackboard import Blackboard
from .coordinator import CoordinatedSwarmCoordinator
from .independent import IndependentWorkerPool

# Alias for backwards compatibility
SwarmCoordinator = CoordinatedSwarmCoordinator

__all__ = ["Blackboard", "CoordinatedSwarmCoordinator", "SwarmCoordinator", "IndependentWorkerPool"]
