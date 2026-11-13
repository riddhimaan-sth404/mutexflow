import os
import json
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Dict, Any, Optional, List
from orchestral_worker.tools.base import BaseSecurityTool
from orchestral_worker.guard.hitl import RiskTier


class CodeQLTool(BaseSecurityTool):
    """
    Integrates GitHub CodeQL semantic code analysis engine.
    Performs data-flow and taint-tracking analysis on codebases,
    compiles QL queries, and converts SARIF outputs into Orchestral findings.
    """

    SUPPORTED_LANGUAGES = ["python", "javascript", "typescript", "go", "java", "cpp", "csharp", "ruby"]

    def __init__(self, codeql_bin_path: Optional[str] = None, **kwargs):
        super().__init__(
            name="codeql_analyzer",
            description="Performs semantic taint-tracking analysis and SARIF vulnerability generation using CodeQL",
            risk_tier=RiskTier.PASSIVE,
            **kwargs,
        )
        self.codeql_bin = codeql_bin_path or shutil.which("codeql") or "codeql"

    def is_available(self) -> bool:
        """Checks if CodeQL CLI is installed and accessible."""
        return shutil.which(self.codeql_bin) is not None

    def _parse_sarif(self, sarif_path: Path) -> List[Dict[str, Any]]:
        """Parses CodeQL SARIF output into normalized Orchestral vulnerability finding format."""
        if not sarif_path.exists():
            return []

        findings = []
        try:
            with open(sarif_path, "r", encoding="utf-8") as f:
                sarif_data = json.load(f)

            for run in sarif_data.get("runs", []):
                rules = {r.get("id"): r for r in run.get("tool", {}).get("driver", {}).get("rules", [])}
                for result in run.get("results", []):
                    rule_id = result.get("ruleId", "CodeQL-Finding")
                    rule_meta = rules.get(rule_id, {})
                    msg = result.get("message", {}).get("text", "CodeQL finding detected")

                    # Extract file location
                    loc = "unknown"
                    locations = result.get("locations", [])
                    if locations:
                        phys = locations[0].get("physicalLocation", {})
                        uri = phys.get("artifactLocation", {}).get("uri", "")
                        line = phys.get("region", {}).get("startLine", 0)
                        loc = f"{uri}:{line}"

                    # Determine severity
                    level = result.get("level", "warning").lower()
                    sev_map = {"error": "HIGH", "warning": "MEDIUM", "note": "LOW"}
                    severity = sev_map.get(level, "MEDIUM")

                    # Security-severity tag override (0.0 - 10.0 scale)
                    props = rule_meta.get("properties", {})
                    sec_sev = props.get("security-severity")
                    cvss = float(sec_sev) if sec_sev else None
                    if cvss and cvss >= 9.0:
                        severity = "CRITICAL"
                    elif cvss and cvss >= 7.0:
                        severity = "HIGH"

                    findings.append({
                        "rule_id": rule_id,
                        "title": rule_meta.get("shortDescription", {}).get("text", rule_id),
                        "description": msg,
                        "severity": severity,
                        "cvss": cvss,
                        "location": loc,
                        "status": "discovered",
                    })
        except Exception as e:
            print(f"[!] Warning: Failed to parse CodeQL SARIF: {e}", file=sys.stderr)

        return findings

    def _run(
        self,
        target: str,
        language: Optional[str] = "python",
        source_dir: Optional[str] = None,
        query_suite: str = "security-extended",
        db_output_dir: Optional[str] = None,
        timeout: int = 900,
        **kwargs,
    ) -> Dict[str, Any]:
        language = (language or "python").lower()
        source_path = Path(source_dir or target)

        if not source_path.exists():
            return {
                "tool": self.name,
                "target": target,
                "status": "source_not_found",
                "error": f"Source path does not exist: {source_path}",
            }

        if not self.is_available():
            return {
                "tool": self.name,
                "target": target,
                "status": "tool_not_installed",
                "message": (
                    "CodeQL CLI binary is not present in PATH. "
                    "Install from https://github.com/github/codeql-action/releases or containerize via "
                    "'mcr.microsoft.com/csttools/codeql-container'."
                ),
                "supported_languages": self.SUPPORTED_LANGUAGES,
            }

        if language not in self.SUPPORTED_LANGUAGES:
            return {
                "tool": self.name,
                "status": "unsupported_language",
                "error": f"Language '{language}' not in supported list: {self.SUPPORTED_LANGUAGES}",
            }

        import uuid

        slug = Path(target).name or "root"
        unique = uuid.uuid4().hex[:8]
        db_dir = Path(db_output_dir or f"/tmp/codeql_db_{slug}_{unique}")
        sarif_out = Path(f"/tmp/codeql_results_{slug}_{unique}.sarif")

        # 1. Create Database
        create_cmd = [
            self.codeql_bin, "database", "create",
            str(db_dir),
            f"--language={language}",
            f"--source-root={source_path}",
            "--overwrite"
        ]

        try:
            subprocess.run(create_cmd, capture_output=True, text=True, check=True, timeout=timeout)
        except subprocess.TimeoutExpired:
            return {
                "tool": self.name,
                "status": "db_create_timeout",
                "error": f"CodeQL database creation timed out after {timeout}s",
            }
        except subprocess.CalledProcessError as e:
            return {
                "tool": self.name,
                "status": "db_create_failed",
                "error": e.stderr.strip() or e.stdout.strip(),
            }

        # 2. Analyze Database
        analyze_cmd = [
            self.codeql_bin, "database", "analyze",
            str(db_dir),
            f"{language}-{query_suite}.qls",
            f"--format=sarif-latest",
            f"--output={sarif_out}",
        ]

        try:
            subprocess.run(analyze_cmd, capture_output=True, text=True, check=True, timeout=timeout)
        except subprocess.TimeoutExpired:
            return {
                "tool": self.name,
                "status": "analysis_timeout",
                "error": f"CodeQL analysis timed out after {timeout}s",
            }
        except subprocess.CalledProcessError as e:
            return {
                "tool": self.name,
                "status": "analysis_failed",
                "error": e.stderr.strip() or e.stdout.strip(),
            }

        # 3. Parse SARIF
        parsed_findings = self._parse_sarif(sarif_out)

        return {
            "tool": self.name,
            "target": target,
            "language": language,
            "query_suite": query_suite,
            "findings_count": len(parsed_findings),
            "findings": parsed_findings,
            "sarif_path": str(sarif_out),
            "database_path": str(db_dir),
        }
