import ipaddress
import os
import re
import shutil
import subprocess
import time
from typing import Dict, Any, Optional, List, Tuple
from orchestral_worker.guard.scope import ScopeGuard, ScopeViolationError
from orchestral_worker.guard.hitl import HITLInterceptor, RiskTier


class KaliSandboxManager:
    """
    Manages an exclusive, hardened Kali Linux container environment for an agent.
    Runs rootless via Podman (or Docker) ensuring terminal security tools (Nmap,
    John the Ripper, Nikto, Trufflehog, Semgrep) are strictly exclusive to the agent
    without contaminating or exposing the host environment.
    
    Hardened with:
    - Dropped Linux capabilities (--cap-drop=ALL, selective additions for socket probes)
    - No-new-privileges flag
    - Strict PID and memory limits
    - Pre-execution argument inspection enforcing ScopeGuard against internal escapes
    - Intelligent Risk-Tiering for IronGate Human-in-the-Loop approvals
    - Non-interactive terminal execution and context-preserving output truncation.
    """

    DEFAULT_IMAGE = "localhost/orchestral-tartarus:latest"

    # Command classification patterns for IronGate Risk Tiers.
    # HIGH_IMPACT commands execute attacks or brute-force credentials.
    HIGH_IMPACT_COMMAND_PATTERNS = [
        re.compile(r"\b(john|hashcat|hydra|medusa|crowbar)\b", re.IGNORECASE),
        re.compile(r"\b(sqlmap|nikto|dirbuster)\b", re.IGNORECASE),
        re.compile(r"\b(msfconsole|msfvenom|metasploit)\b", re.IGNORECASE),
        re.compile(r"\b(nc|ncat|netcat)\b.*-[le]", re.IGNORECASE),
        re.compile(r"\bnmap\b.*--script=(?:exploit|vuln|dos)", re.IGNORECASE),
        re.compile(r"\b(arpspoof|bettercap|ettercap)\b", re.IGNORECASE),
        re.compile(r"\brm\s+-rf\b", re.IGNORECASE),
        re.compile(r"\bdd\s+if=", re.IGNORECASE),
    ]

    # ACTIVE commands touch a remote target but do not exploit it.
    ACTIVE_COMMAND_PATTERNS = [
        re.compile(r"\b(nmap|masscan|zmap)\b", re.IGNORECASE),
        re.compile(r"\b(nuclei|ffuf|gobuster|dirb|wfuzz)\b", re.IGNORECASE),
        re.compile(r"\b(curl|wget|httpie|http)\b", re.IGNORECASE),
        re.compile(r"\b(nc|ncat|netcat|telnet|ssh|ftp)\b", re.IGNORECASE),
        re.compile(r"\b(ping|traceroute|tracepath|dig|nslookup|host)\b", re.IGNORECASE),
        re.compile(r"\b(subfinder|amass|assetfinder|dnsx)\b", re.IGNORECASE),
        re.compile(r"\bopenssl\b.*\bs_client\b", re.IGNORECASE),
    ]

    # Constructs that make static scope validation impossible. They are rejected
    # rather than executed with unvalidated expansion.
    UNSAFE_SHELL_PATTERNS = [
        re.compile(r"\$\("),
        re.compile(r"`"),
        re.compile(r"\$\{"),
        re.compile(r"\beval\b", re.IGNORECASE),
        re.compile(r"\bbash\s+-c\b", re.IGNORECASE),
        re.compile(r"\bsh\s+-c\b", re.IGNORECASE),
    ]
    UNSAFE_INPUT_FILE_PATTERN = re.compile(r"(?:^|\s)(?:-iL|--input-file|-iL=)(?:\s|=)", re.IGNORECASE)

    MAX_OUTPUT_LINES = 100
    MAX_OUTPUT_CHARS = 16000

    def __init__(
        self,
        agent_id: str,
        image: str = DEFAULT_IMAGE,
        scope_guard: Optional[ScopeGuard] = None,
        hitl_interceptor: Optional[HITLInterceptor] = None,
        memory_limit: str = "2g",
        cpu_limit: str = "2.0",
        network_mode: str = "bridge",
    ):
        self.agent_id = agent_id
        self.image = image
        self.scope_guard = scope_guard
        self.hitl = hitl_interceptor
        self.memory_limit = memory_limit
        self.cpu_limit = cpu_limit
        self.network_mode = network_mode
        self.container_name = f"orchestral-kali-{self.agent_id.replace(':', '-').replace('/', '-')}"
        self.runtime = self._detect_runtime()
        self._is_running = False

    def _detect_runtime(self) -> str:
        """Detects whether podman or docker is available, prioritizing rootless podman."""
        if shutil.which("podman"):
            return "podman"
        elif shutil.which("docker"):
            return "docker"
        return "none"

    def start_sandbox(self) -> Dict[str, Any]:
        """Spins up an exclusive, detached, hardened container for this agent."""
        if self.runtime == "none":
            return {
                "status": "error",
                "message": "Neither Podman nor Docker is installed on host.",
            }

        # Check if already running or if stopped container exists
        check_cmd = [self.runtime, "ps", "-a", "--filter", f"name={self.container_name}", "--format", "{{.Status}}"]
        try:
            res = subprocess.run(check_cmd, capture_output=True, text=True, check=True)
            if res.stdout.strip():
                if "Up" in res.stdout:
                    self._is_running = True
                    return {"status": "running", "container": self.container_name, "runtime": self.runtime}
                else:
                    # Stale or exited container; remove so we can spin up clean
                    subprocess.run([self.runtime, "rm", "-f", self.container_name], capture_output=True)
        except Exception:
            pass

        # Create isolated workspace directory on host for agent output
        agent_workspace = f"/tmp/orchestral_workspace_{self.agent_id}"
        os.makedirs(agent_workspace, exist_ok=True)

        # Run container with hardening flags
        run_cmd = [
            self.runtime, "run", "-d",
            "--name", self.container_name,
            "--network", self.network_mode,
            "--memory", self.memory_limit,
            "--cpus", self.cpu_limit,
            "--pids-limit", "150",
            "--ipc", "none",
            "--read-only",
            "--tmpfs", "/tmp:rw,noexec,nosuid,size=256m",
            "--ulimit", "nofile=1024:1024",
            "--security-opt", "no-new-privileges",
            "--cap-drop", "ALL",
            "--cap-add", "NET_BIND_SERVICE",
            "--cap-add", "NET_RAW",
            "-v", f"{agent_workspace}:/workspace:Z",
            "-w", "/workspace",
            self.image,
            "sleep", "infinity"
        ]

        try:
            subprocess.run(run_cmd, capture_output=True, text=True, check=True)
            self._is_running = True
            return {
                "status": "started",
                "container": self.container_name,
                "runtime": self.runtime,
                "workspace": agent_workspace,
                "hardening": ["cap-drop=ALL", "no-new-privileges", "pids-limit=150"],
            }
        except subprocess.CalledProcessError as e:
            return {
                "status": "container_unavailable",
                "error": e.stderr.strip() or e.stdout.strip(),
                "runtime": self.runtime,
                "fallback": "Simulated local sandbox active until image is pulled.",
            }

    @staticmethod
    def classify_risk_tier(command: str) -> RiskTier:
        """Determines the IronGate RiskTier for a given terminal command."""
        cmd_clean = (command or "").strip()
        for pattern in KaliSandboxManager.HIGH_IMPACT_COMMAND_PATTERNS:
            if pattern.search(cmd_clean):
                return RiskTier.HIGH_IMPACT
        for pattern in KaliSandboxManager.ACTIVE_COMMAND_PATTERNS:
            if pattern.search(cmd_clean):
                return RiskTier.ACTIVE
        return RiskTier.PASSIVE

    def _extract_and_validate_all_hosts(self, command: str, explicit_target: str):
        """
        Parses all hostnames, IP addresses, and URLs from the command arguments
        to prevent agents from executing commands that escape the authorized scope
        from inside the container.
        """
        if not self.scope_guard:
            return

        # Reject constructs whose expansion cannot be statically validated.
        for pattern in self.UNSAFE_SHELL_PATTERNS:
            if pattern.search(command):
                raise ScopeViolationError(
                    "Command contains shell substitution/evaluation and cannot be statically scoped."
                )
        if self.UNSAFE_INPUT_FILE_PATTERN.search(command):
            raise ScopeViolationError(
                "Commands reading targets from a file (-iL/--input-file) cannot be scoped."
            )

        # 1. Validate primary explicit target if provided and not a flag
        if explicit_target and not explicit_target.startswith("-") and explicit_target not in (".", "./", "/"):
            self.scope_guard.validate_target(explicit_target)

        # 2. Extract potential IP addresses, domain names, and URLs from the
        # command, including --flag=value and Host: header forms.
        ip_pattern = re.compile(
            r"(?<![\w.])(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}"
            r"(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)(?:/\d{1,2})?(?![\w.])"
        )
        domain_pattern = re.compile(
            r"(?<![\w.-])(?:[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\.)+[a-zA-Z]{2,63}(?![\w.-])"
        )
        url_pattern = re.compile(r"https?://\[?([^/:\]\s]+)\]?")
        host_header_pattern = re.compile(r"(?:^|\s)-H\s+['\"]?Host:\s*([^'\"\s]+)", re.IGNORECASE)
        ipv6_pattern = re.compile(r"(?<![\w:])(?:[0-9a-fA-F]{1,4}:){2,7}[0-9a-fA-F]{1,4}(?![\w:])")

        candidates: List[str] = []
        candidates.extend(url_pattern.findall(command))
        candidates.extend(host_header_pattern.findall(command))

        # Handle --flag=value (e.g. --target=example.com)
        for token in command.strip().split():
            cleaned = token.strip("'\";,()[]{}")
            if "=" in cleaned:
                candidates.append(cleaned.split("=", 1)[1])
            candidates.append(cleaned)

        candidates.extend(ip_pattern.findall(command))
        candidates.extend(domain_pattern.findall(command))
        candidates.extend(ipv6_pattern.findall(command))

        seen = set()
        for cleaned in candidates:
            cleaned = cleaned.strip("'\";,()[]{}")
            if not cleaned or cleaned in seen:
                continue
            seen.add(cleaned)

            # Ignore common non-target flags/options and local paths
            if cleaned.startswith("-") or cleaned in ("http://", "https://", "localhost"):
                continue

            try:
                self.scope_guard.validate_target(cleaned)
            except ScopeViolationError:
                # Only propagate for things that actually look like targets.
                # Ordinary command words (e.g. "bash", "nmap") are not targets.
                if self._looks_like_target(cleaned):
                    raise

    @staticmethod
    def _looks_like_target(value: str) -> bool:
        """Heuristic used to distinguish real targets from ordinary command words."""
        value = value.strip("'\";,()[]{}")
        if not value or value.startswith("-"):
            return False
        if value.startswith(("http://", "https://")):
            return True
        try:
            ipaddress.ip_address(value.split("/", 1)[0])
            return True
        except ValueError:
            pass
        return bool(re.fullmatch(
            r"(?:[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\.)+[a-zA-Z]{2,63}",
            value,
        ))

    def _truncate_output(self, text: str) -> str:
        """Safely truncates terminal output to preserve LLM context budget."""
        if not text:
            return ""
        
        lines = text.splitlines()
        if len(lines) <= self.MAX_OUTPUT_LINES and len(text) <= self.MAX_OUTPUT_CHARS:
            return text

        head_count = self.MAX_OUTPUT_LINES // 2
        tail_count = self.MAX_OUTPUT_LINES // 2
        omitted = len(lines) - (head_count + tail_count)

        head = "\n".join(lines[:head_count])
        tail = "\n".join(lines[-tail_count:]) if tail_count > 0 else ""
        
        return f"{head}\n\n[... Truncated {omitted} lines of terminal output to preserve context budget ...]\n\n{tail}"

    def execute(self, command: str, target: str, timeout: int = 60) -> Dict[str, Any]:
        """
        Executes a Kali terminal tool inside the agent's exclusive sandbox.
        Strictly guarded by deep command argument ScopeGuard and HITL interceptor.
        """
        # 1. Deterministic Scope Validation (Deep argument inspection)
        try:
            self._extract_and_validate_all_hosts(command, target)
        except ScopeViolationError as e:
            return {
                "tool": "kali_terminal",
                "status": "scope_violation",
                "error": str(e),
                "command": command,
                "target": target,
            }

        # 2. Risk-Tiering & Human-in-the-Loop Gate. Every command is routed
        # through the interceptor; it auto-approves anything at or below its
        # configured threshold and blocks the rest.
        tool_name = command.strip().split()[0] if command.strip() else "bash"
        risk_tier = self.classify_risk_tier(command)

        if self.hitl:
            approved = self.hitl.request_approval(
                agent_id=self.agent_id,
                tool_name=f"kali:{tool_name}",
                target=target,
                command=command,
                risk_tier=risk_tier,
            )
            if not approved:
                return {
                    "tool": f"kali:{tool_name}",
                    "status": "denied",
                    "error": f"Operator denied terminal command '{command}' on target '{target}'.",
                    "risk_tier": risk_tier.value,
                }

        # 3. Execute inside container with non-interactive environment
        start_time = time.time()
        env_prefix = "CI=1 DEBIAN_FRONTEND=noninteractive TERM=dumb "

        if not self._is_running or self.runtime == "none":
            # Fail closed. Reporting a fake success here would hide the fact
            # that no security tool actually ran.
            return {
                "tool": f"kali:{tool_name}",
                "status": "sandbox_unavailable",
                "error": (
                    "No container runtime or sandbox is available; refusing to "
                    "simulate command execution."
                ),
                "command": command,
                "target": target,
                "risk_tier": risk_tier.value,
            }

        exec_cmd = [self.runtime, "exec", self.container_name, "bash", "-c", f"{env_prefix}{command}"]
        try:
            proc = subprocess.run(exec_cmd, capture_output=True, text=True, timeout=timeout)
            duration = round(time.time() - start_time, 2)
            return {
                "tool": f"kali:{tool_name}",
                "container": self.container_name,
                "command": command,
                "target": target,
                "exit_code": proc.returncode,
                "stdout": self._truncate_output(proc.stdout.strip()),
                "stderr": self._truncate_output(proc.stderr.strip()),
                "duration_sec": duration,
                "risk_tier": risk_tier.value,
                "sandbox": "hardened_podman_container",
            }
        except subprocess.TimeoutExpired:
            return {
                "tool": f"kali:{tool_name}",
                "status": "timeout",
                "error": f"Command timed out after {timeout}s",
            }

    def stop_sandbox(self):
        """Destroys the exclusive container upon agent termination."""
        if self._is_running and self.runtime != "none":
            subprocess.run([self.runtime, "rm", "-f", self.container_name], capture_output=True)
            self._is_running = False
