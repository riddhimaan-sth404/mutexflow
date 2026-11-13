import os
import re
from typing import Dict, Any, List
from orchestral_worker.tools.base import BaseSecurityTool
from orchestral_worker.guard.hitl import RiskTier


class NativeSecretScannerTool(BaseSecurityTool):
    """
    Scans source files, repositories, or config text for hardcoded secrets,
    cloud credentials (AWS, GCP, Azure, GitHub), private keys, and API tokens.
    """

    PATTERNS = {
        "AWS Access Key": r"\b(AKIA[0-9A-Z]{16})\b",
        # A 40-char key only counts when it appears next to an AWS secret-key
        # identifier; otherwise every long alphanumeric string is a false positive.
        "AWS Secret Key": (
            r"(?i)(?:aws_secret_access_key|aws_secret_key|secret_access_key)"
            r"\s*[:=]\s*['\"]?([0-9a-zA-Z/+]{40})['\"]?"
        ),
        "Generic Private Key": r"-----BEGIN (RSA|EC|DSA|OPENSSH|PRIVATE) KEY-----",
        "GitHub Personal Token": r"\b(ghp_[0-9a-zA-Z]{36}|github_pat_[0-9a-zA-Z_]{82})\b",
        "Slack Webhook / Token": r"https://hooks\.slack\.com/services/T[0-9a-zA-Z]+/B[0-9a-zA-Z]+/[0-9a-zA-Z]+",
        "Google Cloud API Key": r"\b(AIza[0-9A-Za-z\\-_]{35})\b",
        "JWT Token": r"\beyJ[A-Za-z0-9-_]+\.eyJ[A-Za-z0-9-_]+\.[A-Za-z0-9-_]+\b",
    }

    SCAN_EXTENSIONS = (
        ".py", ".js", ".ts", ".tsx", ".jsx", ".go", ".rs", ".rb", ".java",
        ".json", ".env", ".yaml", ".yml", ".conf", ".ini", ".toml", ".sh",
    )
    IGNORED_DIRS = {
        ".git", ".hg", ".svn", "node_modules", "target", "obj", "bin",
        "dist", "build", ".venv", "venv", "__pycache__", "goose-upstream",
        "orchestral-worker.egg-info", ".mypy_cache", ".pytest_cache",
    }

    # The scanner runs as the operator's user, so it must not be usable as a
    # generic host-file reader. These prefixes are never scanned, even if a
    # caller passes them explicitly. Paths are compared after realpath() so a
    # symlink cannot be used to escape the deny-list.
    DENIED_PREFIXES = (
        "/etc",
        "/proc",
        "/sys",
        "/root",
        "/dev",
        "/boot",
        "/run",
        "/var/lib",
        "/var/run",
        "/usr/lib/systemd",
        "/snap",
    )
    # Sensitive credential directories/files, matched on any path component.
    DENIED_NAME_PARTS = {
        ".ssh", ".aws", ".gnupg", ".kube", ".docker", ".azure",
        ".netrc", ".npmrc", ".pypirc", ".pgpass", ".git-credentials",
        "id_rsa", "id_dsa", "id_ecdsa", "id_ed25519",
        "authorized_keys", "known_hosts",
    }

    MAX_FILE_BYTES = 2 * 1024 * 1024

    def __init__(self, **kwargs):
        super().__init__(
            name="native_secret_scanner",
            description="Detects hardcoded secrets, API tokens, and cryptographic keys in source code",
            risk_tier=RiskTier.PASSIVE,
            **kwargs,
        )

    def _scan_text(self, text: str, source_identifier: str) -> List[Dict[str, Any]]:
        findings = []
        for secret_type, pattern in self.PATTERNS.items():
            for match in re.finditer(pattern, text):
                matched_val = match.group(0)
                # Mask value
                masked = matched_val[:4] + "*" * (len(matched_val) - 8) + matched_val[-4:] if len(matched_val) > 8 else "***"
                findings.append({
                    "type": secret_type,
                    "source": source_identifier,
                    "preview": masked,
                    "severity": "CRITICAL",
                })
        return findings

    def _is_denied(self, path: str) -> bool:
        """Return True when a resolved path must not be read.

        This blocks sensitive system trees and credential directories. It is a
        read guard, not an access-control boundary: the process still runs as
        the operator, so the goal is to avoid turning a scan target into an
        arbitrary host-file read.
        """
        try:
            real = os.path.realpath(path)
        except OSError:
            return True
        for prefix in self.DENIED_PREFIXES:
            if real == prefix or real.startswith(prefix + os.sep):
                return True
        parts = set(real.split(os.sep))
        if parts & self.DENIED_NAME_PARTS:
            return True
        # Never follow a symlink that points outside the requested root.
        return False

    def _resolve_scan_root(self, candidate: str) -> str:
        """Resolve a scan root, rejecting denied or nonexistent paths."""
        real = os.path.realpath(candidate)
        if real == os.sep:
            return ""
        if not os.path.exists(real):
            return ""
        if self._is_denied(real):
            return ""
        return real

    def _run(self, target: str, content: str = None, directory_path: str = None, **kwargs) -> Dict[str, Any]:
        all_findings = []
        scan_root = directory_path
        denied_paths: List[str] = []

        # Allow target itself to be a directory, which is how the deterministic
        # runner and the MCP secret_scan tool invoke this.
        if not scan_root and target and os.path.isdir(target):
            scan_root = target

        if content:
            all_findings.extend(self._scan_text(content, f"raw_buffer:{target}"))

        if scan_root:
            if not os.path.exists(scan_root):
                return {
                    "tool": self.name,
                    "target": target,
                    "scanned_path": scan_root,
                    "status": "error",
                    "error": f"Scan path does not exist: {scan_root}",
                    "findings_count": 0,
                    "findings": [],
                }
            resolved_root = self._resolve_scan_root(scan_root)
            if not resolved_root:
                return {
                    "tool": self.name,
                    "target": target,
                    "scanned_path": os.path.realpath(scan_root),
                    "status": "denied",
                    "error": (
                        "Refusing to scan a protected path. The secret scanner "
                        "may only read project/workspace files, not system or "
                        "credential directories."
                    ),
                    "findings_count": 0,
                    "findings": [],
                }
            scan_root = resolved_root
            root_prefix = scan_root + os.sep
            for root, dirs, files in os.walk(scan_root):
                dirs[:] = [
                    d for d in dirs
                    if d not in self.IGNORED_DIRS
                    and not self._is_denied(os.path.join(root, d))
                ]
                for f in files:
                    if not f.endswith(self.SCAN_EXTENSIONS):
                        continue
                    fpath = os.path.join(root, f)
                    # Do not follow symlinks; they are an easy escape hatch.
                    if os.path.islink(fpath):
                        continue
                    if not os.path.realpath(fpath).startswith(root_prefix):
                        denied_paths.append(fpath)
                        continue
                    if self._is_denied(fpath):
                        denied_paths.append(fpath)
                        continue
                    try:
                        if os.path.getsize(fpath) > self.MAX_FILE_BYTES:
                            continue
                        with open(fpath, "r", encoding="utf-8", errors="ignore") as fh:
                            text = fh.read()
                            all_findings.extend(self._scan_text(text, fpath))
                    except Exception:
                        continue

        result: Dict[str, Any] = {
            "tool": self.name,
            "target": target,
            "scanned_path": scan_root,
            "findings_count": len(all_findings),
            "findings": all_findings,
        }
        if denied_paths:
            result["skipped_protected_paths"] = len(denied_paths)
        return result
