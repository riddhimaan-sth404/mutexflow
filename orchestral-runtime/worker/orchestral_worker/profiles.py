import os
import yaml
from pathlib import Path
from typing import Dict, Any, List, Optional

SWARM_ROOT = Path(__file__).resolve().parent.parent.parent
# Allow an explicit recipes location when the worker is installed outside the
# source checkout (for example from a wheel).
RECIPES_DIR = Path(os.environ.get("ORCHESTRAL_RECIPES_DIR", SWARM_ROOT / "recipes"))


class AgentProfile:
    """Represents an AI agent personality and operational instruction profile mapped from Goose recipes."""

    def __init__(self, key: str, title: str, description: str, instructions: str, parameters: List[Dict[str, Any]]):
        self.key = key
        self.name = title
        self.id = key
        self.role = title
        self.description = description
        self.instructions = instructions
        self.parameters = parameters
        self.mode = "coordinated"
        self.default_risk_tier = "passive"
        self.permitted_tools = ["tartarus_exec", "secret_scan", "codeql_analyze", "nexus_record_service", "nexus_query_state"]

    def get_full_prompt(self, target: Optional[str] = None, scope: Optional[List[str]] = None) -> str:
        prompt = self.instructions
        if target:
            prompt = prompt.replace("{{target}}", target)
        if scope:
            prompt = prompt.replace("{{scope}}", ", ".join(scope))
        return prompt


def parse_recipe_file(recipe_dir: Path) -> Optional[AgentProfile]:
    recipe_yaml = recipe_dir / "recipe.yaml"
    if not recipe_yaml.exists():
        return None

    try:
        data = yaml.safe_load(recipe_yaml.read_text(encoding="utf-8"))
        if not isinstance(data, dict):
            return None
        return AgentProfile(
            key=recipe_dir.name,
            title=data.get("title", recipe_dir.name),
            description=data.get("description", ""),
            instructions=data.get("instructions", ""),
            parameters=data.get("parameters", []),
        )
    except Exception:
        return None


def list_profiles() -> List[AgentProfile]:
    if not RECIPES_DIR.exists():
        return []
    profiles = []
    for d in sorted(RECIPES_DIR.iterdir()):
        if d.is_dir():
            p = parse_recipe_file(d)
            if p:
                profiles.append(p)
    return profiles


def get_profile(name_or_id: str) -> Optional[AgentProfile]:
    query = name_or_id.lower().strip().replace("-", "_")
    for p in list_profiles():
        search_fields = [p.name.lower(), p.key.lower(), p.id.lower(), p.role.lower()]
        if any(query in field for field in search_fields):
            return p
    return None
