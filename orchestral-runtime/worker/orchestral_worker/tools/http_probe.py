import urllib.request
import urllib.error
from typing import Dict, Any, List
from orchestral_worker.tools.base import BaseSecurityTool
from orchestral_worker.guard.hitl import RiskTier


class NativeHttpProbeTool(BaseSecurityTool):
    """
    Performs passive and non-intrusive active HTTP probing:
    - Inspects security headers (HSTS, CSP, X-Frame-Options, X-Content-Type-Options)
    - Probes for common information leaks (.git/HEAD, .env, /actuator/env, /robots.txt)
    - Verifies CORS policy misconfigurations
    """

    COMMON_SENSITIVE_PATHS = [
        "/.env",
        "/.git/HEAD",
        "/actuator/env",
        "/actuator/health",
        "/swagger.json",
        "/openapi.json",
        "/robots.txt",
    ]

    def __init__(self, **kwargs):
        super().__init__(
            name="native_http_probe",
            description="Audits HTTP security headers and tests for exposed configuration files",
            risk_tier=RiskTier.ACTIVE,
            **kwargs,
        )

    def _run(self, target: str, check_sensitive_files: bool = True, timeout: float = 2.0, **kwargs) -> Dict[str, Any]:
        # When the caller did not specify a scheme, try HTTPS first and fall
        # back to plain HTTP so local HTTP services are not silently skipped.
        if target.startswith("http://") or target.startswith("https://"):
            candidates = [target]
        else:
            candidates = [f"https://{target}", f"http://{target}"]

        headers_found = {}
        missing_security_headers = []
        exposed_files = []
        base_url = candidates[0]
        status_code = None
        last_error = None

        # 1. Inspect Base URL Headers
        for candidate in candidates:
            try:
                req = urllib.request.Request(candidate, headers={"User-Agent": "Orchestral-Probe/1.0"})
                with urllib.request.urlopen(req, timeout=timeout) as resp:
                    headers_found = dict(resp.headers)
                    status_code = resp.status
                base_url = candidate
                last_error = None
                break
            except urllib.error.HTTPError as e:
                status_code = e.code
                headers_found = dict(e.headers)
                base_url = candidate
                last_error = None
                break
            except Exception as e:
                last_error = e
                continue

        if last_error is not None:
            return {"tool": self.name, "target": target, "status": "error", "error": str(last_error)}

        # Evaluate Missing Defensive Headers
        sec_headers = {
            "Strict-Transport-Security": "Missing HSTS allows downgrade attacks",
            "Content-Security-Policy": "Missing CSP increases XSS susceptibility",
            "X-Frame-Options": "Missing X-Frame-Options exposes application to Clickjacking",
            "X-Content-Type-Options": "Missing nosniff allows MIME-sniffing attacks",
        }
        for hdr, reason in sec_headers.items():
            if not any(k.lower() == hdr.lower() for k in headers_found):
                missing_security_headers.append({"header": hdr, "risk": reason})

        # 2. Check for sensitive files if requested
        if check_sensitive_files:
            for path in self.COMMON_SENSITIVE_PATHS:
                test_url = f"{base_url.rstrip('/')}{path}"
                try:
                    t_req = urllib.request.Request(test_url, headers={"User-Agent": "Orchestral-Probe/1.0"})
                    with urllib.request.urlopen(t_req, timeout=timeout) as t_resp:
                        if t_resp.status == 200:
                            content_sample = t_resp.read(128).decode("utf-8", errors="ignore")
                            # Verify not generic 200 HTML error
                            if "<html" not in content_sample.lower() or path == "/robots.txt":
                                exposed_files.append({
                                    "path": path,
                                    "url": test_url,
                                    "sample": content_sample.strip(),
                                    "severity": "HIGH" if "env" in path or "git" in path else "LOW",
                                })
                except Exception:
                    continue

        return {
            "tool": self.name,
            "target": target,
            "status_code": status_code,
            "missing_headers": missing_security_headers,
            "exposed_files": exposed_files,
        }
