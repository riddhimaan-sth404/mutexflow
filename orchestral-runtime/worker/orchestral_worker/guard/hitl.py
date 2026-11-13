import enum
import re
import sys
import time
from typing import Optional, Callable, Any


class RiskTier(str, enum.Enum):
    PASSIVE = "passive"          # OSINT, static code analysis, local file inspection
    ACTIVE = "active"            # Port scanning, banner grabs, HTTP option queries
    HIGH_IMPACT = "high_impact"  # Active exploits, auth fuzzing, SQLi payload execution


class HITLDecision(str, enum.Enum):
    PENDING = "pending"
    APPROVED = "approved"
    DENIED = "denied"
    TIMEOUT = "timeout"


# Tools that are inherently passive: they inspect local files/artifacts and never
# send traffic to a target. These must never block on human approval.
PASSIVE_TOOL_NAMES = {
    "native_secret_scanner",
    "secret_scanner",
    "native_static_analyzer",
    "codeql",
    "semgrep",
    "gitleaks",
    "trivy",
    "dependency_audit",
    "file_reader",
    "read_file",
}

# Tools that actively touch a target over the network.
ACTIVE_TOOL_NAMES = {
    "kali_terminal",
    "nmap",
    "naabu",
    "nuclei",
    "ffuf",
    "gobuster",
    "curl",
    "http_probe",
    "native_http_probe",
    "native_port_scanner",
    "port_scanner",
    "fuzzer",
    "subfinder",
    "amass",
}

# Tools that execute attacks / mutate state.
HIGH_IMPACT_TOOL_NAMES = {
    "exploit",
    "metasploit",
    "sqlmap",
    "hydra",
    "bruteforce",
    "msfconsole",
}

# Word-boundary command patterns. Matching on substrings caused false positives
# such as "scan" inside a tool name or "write" inside "curl -w".
_HIGH_IMPACT_PATTERNS = [
    r"\bexploit\b",
    r"\bpayload\b",
    r"\bsqlmap\b",
    r"\bmetasploit\b",
    r"\bmsfconsole\b",
    r"\bhydra\b",
    r"\bbruteforce\b",
    r"\bbrute-force\b",
    r"\binject(?:ion)?\b",
    r"\bdrop\s+table\b",
    r"\breverse_tcp\b",
    r"\breverse\s+shell\b",
    r"\brm\s+-rf\b",
    r"\bdd\s+if=",
    r"\bmkfs\b",
]

_ACTIVE_PATTERNS = [
    r"\bnmap\b",
    r"\bnaabu\b",
    r"\bnuclei\b",
    r"\bffuf\b",
    r"\bgobuster\b",
    r"\bcurl\b",
    r"\bwget\b",
    r"\bnikto\b",
    r"\bmasscan\b",
    r"\bprobe\b",
    r"\bport\s*scan\b",
    r"\bsubfinder\b",
    r"\bamass\b",
]


class HITLInterceptor:
    """
    Human-in-the-Loop Gateway for Security Swarms.
    Prevents autonomous agents from executing high-risk attacks without
    explicit human sign-off via the Monitor UI or API.

    When an action requires approval:
    1. Dispatches HITL request to Orchestral Citadel daemon.
    2. Enters a synchronous blocking poll loop on the Core API until an operator
       approves or denies the action, or until timeout.
    3. Fails closed (DENY) on timeout or error.
    """

    def __init__(
        self,
        auto_approve_tier: RiskTier = RiskTier.PASSIVE,
        client: Optional[Any] = None,
        approval_callback: Optional[Callable[[str, str, str, RiskTier], bool]] = None,
        timeout_seconds: int = 120,
    ):
        self.auto_approve_tier = auto_approve_tier
        self.client = client
        self.approval_callback = approval_callback
        self.timeout_seconds = timeout_seconds

    @staticmethod
    def classify_tool(tool_name: str) -> Optional[RiskTier]:
        """Classify a tool by its exact name, not by substrings.

        Returns None when the tool is unknown so command inspection can decide.
        """
        name = (tool_name or "").strip().lower()
        if name in HIGH_IMPACT_TOOL_NAMES:
            return RiskTier.HIGH_IMPACT
        if name in PASSIVE_TOOL_NAMES:
            return RiskTier.PASSIVE
        if name in ACTIVE_TOOL_NAMES:
            return RiskTier.ACTIVE
        return None

    def evaluate_risk(
        self,
        command: str,
        tool_name: str,
        risk_tier: Optional[RiskTier] = None,
    ) -> RiskTier:
        """Determine the risk tier for an action.

        An explicit ``risk_tier`` (supplied by the tool itself) always wins.
        Otherwise the tool name is matched exactly, and only then is the command
        inspected using word-boundary patterns.
        """
        if risk_tier is not None:
            return RiskTier(risk_tier)

        tool_tier = self.classify_tool(tool_name)
        if tool_tier is not None:
            return tool_tier

        cmd_lower = (command or "").lower()

        if any(re.search(p, cmd_lower) for p in _HIGH_IMPACT_PATTERNS):
            return RiskTier.HIGH_IMPACT

        if any(re.search(p, cmd_lower) for p in _ACTIVE_PATTERNS):
            return RiskTier.ACTIVE

        # Default to Passive (e.g., semgrep, static ast, local file reading)
        return RiskTier.PASSIVE

    def request_approval(
        self,
        agent_id: str,
        tool_name: str,
        target: str,
        command: str,
        risk_tier: Optional[RiskTier] = None,
    ) -> bool:
        risk = self.evaluate_risk(command, tool_name, risk_tier)

        # Auto-approve if risk is within auto-approval threshold
        if self.auto_approve_tier == RiskTier.ACTIVE:
            if risk in (RiskTier.PASSIVE, RiskTier.ACTIVE):
                return True
        elif self.auto_approve_tier == RiskTier.HIGH_IMPACT:
            return True
        elif self.auto_approve_tier == RiskTier.PASSIVE:
            if risk == RiskTier.PASSIVE:
                return True

        # High impact or exceeding threshold requires external approval
        _log(f"\n[HITL GATE] Agent '{agent_id}' requested {risk.value.upper()} action:")
        _log(f"  Target:  {target}")
        _log(f"  Tool:    {tool_name}")
        _log(f"  Command: {command}")

        # Path 1: Remote operator approval via Orchestral Citadel API (Persistent Blocking Loop)
        if self.client is not None:
            try:
                hitl_res = self.client.request_hitl(
                    agent_id=agent_id,
                    tool_name=tool_name,
                    target=target,
                    command_preview=command,
                    risk_tier=risk.value,
                )
                hitl_id = hitl_res.get("id")
                _log(f"  --> HITL Request registered in Orchestral Citadel (ID: {hitl_id})")
                _log(f"  --> Waiting for operator approval via TUI or `orchestral hitl approve {hitl_id}`...")

                start_time = time.time()
                while time.time() - start_time < self.timeout_seconds:
                    time.sleep(1.5)
                    try:
                        record = self.client.get_hitl_request(hitl_id)
                        status = record.get("status", "").lower()
                        if status == "approved":
                            _log(f"[HITL GATE] Request {hitl_id} APPROVED by operator.")
                            return True
                        elif status == "denied":
                            _log(f"[HITL GATE] Request {hitl_id} DENIED by operator.")
                            return False
                    except Exception:
                        # Log error and retry until timeout
                        pass

                _log(f"[HITL GATE] Request {hitl_id} TIMED OUT after {self.timeout_seconds}s. Failing closed (DENIED).")
                return False
            except Exception as e:
                _log(f"[HITL GATE] Failed to dispatch HITL request to Orchestral Citadel: {e}")

        # Path 2: In-process callback
        if self.approval_callback:
            return self.approval_callback(agent_id, tool_name, target, risk)

        # Path 3: Direct terminal interactive fallback if running with stdin attached
        if sys.stdin.isatty():
            try:
                ans = input(f"Approve execution of '{command}' on {target}? [y/N]: ").strip().lower()
                return ans in ("y", "yes")
            except EOFError:
                return False

        # Fail closed
        _log("[HITL GATE] No operator response channel available. Action DENIED.")
        return False


def _log(message: str) -> None:
    """Write HITL diagnostics to stderr so MCP stdout stays valid JSON-RPC."""
    print(message, file=sys.stderr)
