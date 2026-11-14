"""Loopback-only, local-workspace analysis service for MutexFlow.

This service deliberately exposes only passive source-tree secret scanning. It
does not expose the autonomous network/security tool registry or shell tools.
"""
from __future__ import annotations

import hmac
import json
import os
import threading
import time
import urllib.request
import uuid
from concurrent.futures import ThreadPoolExecutor
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

from orchestral_worker.tools.secret_scanner import NativeSecretScannerTool

HOST = "127.0.0.1"
PORT = int(os.environ.get("ORCHESTRAL_WORKER_PORT", "8090"))
TOKEN = os.environ.get("MUTEXFLOW_WORKER_TOKEN", "")
BACKEND_URL = os.environ.get("MUTEXFLOW_BACKEND_URL", "http://127.0.0.1:3001").rstrip("/")
WORKSPACE_ROOT = Path(os.environ.get("MUTEXFLOW_WORKSPACE_ROOT", os.getcwd())).resolve()
AGENT_ID = "mutexflow-local-analysis"
MAX_BODY = 16 * 1024
JOB_TTL_SECONDS = float(os.environ.get("MUTEXFLOW_JOB_TTL_SECONDS", "3600"))
ACTIVE_STATUSES = ("queued", "running", "cancel_requested")

# Findings severity is derived from the matched pattern instead of a blanket
# CRITICAL/9.0: long-lived provider credentials rank highest, scoped or
# short-lived credentials rank lower, and unknown patterns stay conservative.
SEVERITY_BY_PATTERN: dict[str, tuple[str, float]] = {
    "AWS Access Key": ("HIGH", 7.5),
    "AWS Secret Key": ("HIGH", 7.5),
    "Generic Private Key": ("HIGH", 7.5),
    "GitHub Personal Token": ("HIGH", 7.5),
    "Google Cloud API Key": ("MEDIUM", 5.5),
    "Slack Webhook / Token": ("MEDIUM", 5.0),
    "JWT Token": ("MEDIUM", 5.0),
}
FALLBACK_SEVERITY: tuple[str, float] = ("LOW", 3.1)


def _severity_for(pattern_type: str) -> tuple[str, float]:
    return SEVERITY_BY_PATTERN.get(pattern_type, FALLBACK_SEVERITY)

_jobs: dict[str, dict[str, Any]] = {}
_jobs_lock = threading.Lock()
_cancel_events: dict[str, threading.Event] = {}
_executor = ThreadPoolExecutor(max_workers=2, thread_name_prefix="mutexflow-analysis")


def _json_bytes(value: Any) -> bytes:
    return json.dumps(value, separators=(",", ":"), default=str).encode("utf-8")


def _resolve_workspace(relative: str) -> Path:
    candidate = Path(relative)
    if candidate.is_absolute() or candidate.drive or candidate.root or any(part == ".." for part in candidate.parts):
        raise ValueError("workspace must be a relative path inside the configured workspace root")
    resolved = (WORKSPACE_ROOT / candidate).resolve(strict=True)
    if os.path.commonpath((str(WORKSPACE_ROOT), str(resolved))) != str(WORKSPACE_ROOT):
        raise ValueError("workspace resolves outside the configured workspace root")
    if not resolved.is_dir():
        raise ValueError("workspace must be an existing directory")
    if resolved == Path(resolved.anchor):
        raise ValueError("filesystem root cannot be scanned")
    return resolved


def _submit_finding(finding: dict[str, Any]) -> None:
    """Submit one finding through the MutexFlow backend proxy. The worker
    authenticates with its restricted token and never holds the Citadel
    admin key; the backend attaches the privileged key server-side."""
    body = json.dumps({"findings": finding}, separators=(",", ":")).encode("utf-8")
    request = urllib.request.Request(
        f"{BACKEND_URL}/api/v1/orchestral/findings/submit",
        data=body,
        method="POST",
        headers={
            "Content-Type": "application/json",
            "X-MutexFlow-Worker-Token": TOKEN,
        },
    )
    with urllib.request.urlopen(request, timeout=30):
        pass


def _prune_stale_jobs() -> None:
    """Drop finished job records and their cancel events after the TTL so the
    registries cannot grow without bound over a long-lived session."""
    now = time.monotonic()
    with _jobs_lock:
        for job_id in list(_jobs.keys()):
            job = _jobs[job_id]
            if job.get("status") in ACTIVE_STATUSES:
                continue
            if now - job.get("finished_at", now) > JOB_TTL_SECONDS:
                _jobs.pop(job_id, None)
                _cancel_events.pop(job_id, None)


def _run_job(job_id: str, relative: str, cancel_event: threading.Event) -> None:
    with _jobs_lock:
        _jobs[job_id]["status"] = "running"
    try:
        workspace = _resolve_workspace(relative)
        scanner = NativeSecretScannerTool()
        result = scanner.execute(
            agent_id=AGENT_ID,
            target=str(workspace),
            directory_path=str(workspace),
            cancel_event=cancel_event,
        )
        findings = []
        for item in result.get("findings", []):
            source = str(item.get("source", ""))
            try:
                source = str(Path(source).resolve().relative_to(WORKSPACE_ROOT))
            except (ValueError, OSError):
                source = Path(source).name
            severity, cvss = _severity_for(str(item.get("type", "")))
            findings.append({
                "title": str(item.get("type", "Potential secret detected")),
                "target": source,
                "severity": severity,
                "cvss": cvss,
                "description": "A possible hard-coded credential pattern was detected. The value is masked and is not included in this result.",
                "evidence": str(item.get("preview", "masked")),
            })

        # Persist each finding through the MutexFlow backend proxy. The worker
        # authenticates with its restricted token and never holds the Citadel
        # admin key; the backend attaches the privileged key server-side. No
        # secret material is transmitted; evidence contains only the mask.
        submitted = 0
        for finding in findings:
            payload = {
                "agent_id": AGENT_ID,
                "target": finding["target"],
                "title": finding["title"],
                "severity": finding["severity"],
                "cvss": finding["cvss"],
                "cve": None,
                "description": finding["description"],
                "evidence": finding["evidence"],
                "status": "discovered",
            }
            _submit_finding(payload)
            submitted += 1

        with _jobs_lock:
            _jobs[job_id].update({
                "status": "cancelled" if result.get("status") == "cancelled" else "completed",
                "finished_at": time.monotonic(),
                "workspace": relative,
                "scanned_files_findings": len(findings),
                "findings_submitted": submitted,
                "findings": findings,
            })
    except Exception as exc:  # surfaced as a bounded job error
        with _jobs_lock:
            _jobs[job_id].update({"status": "failed", "finished_at": time.monotonic(), "error": f"{type(exc).__name__}: {exc}"})
    finally:
        with _jobs_lock:
            _cancel_events.pop(job_id, None)


class Handler(BaseHTTPRequestHandler):
    server_version = "MutexFlowWorker/1.0"

    def log_message(self, fmt: str, *args: Any) -> None:
        # Avoid logging request bodies, auth headers, or scanned content.
        print("[MutexFlow worker] " + fmt % args)

    def _send(self, status: int, value: Any) -> None:
        body = _json_bytes(value)
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def _authorized(self) -> bool:
        supplied = self.headers.get("X-MutexFlow-Worker-Token", "")
        return bool(TOKEN) and hmac.compare_digest(supplied, TOKEN)

    def do_GET(self) -> None:  # noqa: N802
        if self.path == "/health":
            self._send(200, {"status": "ok", "service": "mutexflow-local-analysis"})
            return
        if not self._authorized():
            self._send(401, {"error": "unauthorized"})
            return
        _prune_stale_jobs()
        if self.path.startswith("/v1/jobs/"):
            job_id = self.path.rsplit("/", 1)[-1]
            with _jobs_lock:
                job = _jobs.get(job_id)
                if job is None:
                    self._send(404, {"error": "job not found"})
                else:
                    self._send(200, dict(job))
            return
        self._send(404, {"error": "not found"})

    def do_POST(self) -> None:  # noqa: N802
        if not self._authorized():
            self._send(401, {"error": "unauthorized"})
            return
        if self.path.startswith("/v1/jobs/") and self.path.endswith("/cancel"):
            job_id = self.path[len("/v1/jobs/"):-len("/cancel")].strip("/")
            with _jobs_lock:
                job = _jobs.get(job_id)
                event = _cancel_events.get(job_id)
                if job is None or event is None:
                    self._send(404, {"error": "job not found or already finished"})
                    return
                if job.get("status") not in ("queued", "running", "cancel_requested"):
                    self._send(409, {"error": "job is no longer cancellable"})
                    return
                event.set()
                job["status"] = "cancel_requested"
            self._send(202, {"job_id": job_id, "status": "cancel_requested"})
            return

        if self.path != "/v1/jobs/local-analysis":
            self._send(404, {"error": "not found"})
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if length <= 0 or length > MAX_BODY:
                self._send(413, {"error": "request body must be between 1 and 16384 bytes"})
                return
            payload = json.loads(self.rfile.read(length))
            relative = payload.get("workspace", ".")
            if not isinstance(relative, str):
                raise ValueError("workspace must be a relative path string")
            _resolve_workspace(relative)
        except (ValueError, OSError, json.JSONDecodeError) as exc:
            self._send(400, {"error": str(exc)})
            return

        _prune_stale_jobs()
        job_id = str(uuid.uuid4())
        with _jobs_lock:
            _jobs[job_id] = {"job_id": job_id, "status": "queued", "workspace": relative}
            cancel_event = threading.Event()
            _cancel_events[job_id] = cancel_event
        _executor.submit(_run_job, job_id, relative, cancel_event)
        self._send(202, {"job_id": job_id, "status": "queued"})

    def do_OPTIONS(self) -> None:  # noqa: N802
        self._send(405, {"error": "method not allowed"})


def main() -> None:
    if not TOKEN:
        raise SystemExit("MUTEXFLOW_WORKER_TOKEN must be configured before starting the worker")
    WORKSPACE_ROOT.mkdir(parents=True, exist_ok=True)
    server = ThreadingHTTPServer((HOST, PORT), Handler)
    server.daemon_threads = True
    print(f"[MutexFlow worker] listening on http://{HOST}:{PORT}")
    try:
        server.serve_forever(poll_interval=0.5)
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
        _executor.shutdown(wait=False, cancel_futures=True)


if __name__ == "__main__":
    main()
