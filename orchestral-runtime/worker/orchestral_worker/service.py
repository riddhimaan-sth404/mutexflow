"""Loopback-only, local-workspace analysis service for MutexFlow.

This service deliberately exposes only passive source-tree secret scanning. It
does not expose the autonomous network/security tool registry or shell tools.
"""
from __future__ import annotations

import hmac
import json
import os
import threading
import uuid
from concurrent.futures import ThreadPoolExecutor
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

from orchestral_worker.tools.secret_scanner import NativeSecretScannerTool

HOST = "127.0.0.1"
PORT = int(os.environ.get("ORCHESTRAL_WORKER_PORT", "8090"))
TOKEN = os.environ.get("ORCHESTRAL_ADMIN_KEY", "")
CORE_URL = os.environ.get("ORCHESTRAL_CORE_URL", "http://127.0.0.1:8088")
WORKSPACE_ROOT = Path(os.environ.get("MUTEXFLOW_WORKSPACE_ROOT", os.getcwd())).resolve()
AGENT_ID = "mutexflow-local-analysis"
MAX_BODY = 16 * 1024

_jobs: dict[str, dict[str, Any]] = {}
_jobs_lock = threading.Lock()
_cancel_events: dict[str, threading.Event] = {}
_executor = ThreadPoolExecutor(max_workers=2, thread_name_prefix="mutexflow-analysis")


def _json_bytes(value: Any) -> bytes:
    return json.dumps(value, separators=(",", ":"), default=str).encode("utf-8")


def _resolve_workspace(relative: str) -> Path:
    candidate = Path(relative)
    if candidate.is_absolute() or any(part == ".." for part in candidate.parts):
        raise ValueError("workspace must be a relative path inside the configured workspace root")
    resolved = (WORKSPACE_ROOT / candidate).resolve(strict=True)
    if os.path.commonpath((str(WORKSPACE_ROOT), str(resolved))) != str(WORKSPACE_ROOT):
        raise ValueError("workspace resolves outside the configured workspace root")
    if not resolved.is_dir():
        raise ValueError("workspace must be an existing directory")
    if resolved == Path(resolved.anchor):
        raise ValueError("filesystem root cannot be scanned")
    return resolved


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
            findings.append({
                "title": str(item.get("type", "Potential secret detected")),
                "target": source,
                "severity": "CRITICAL",
                "cvss": 9.0,
                "description": "A possible hard-coded credential pattern was detected. The value is masked and is not included in this result.",
                "evidence": str(item.get("preview", "masked")),
            })

        # Persist each finding through Citadel's authenticated API. No secret
        # material is transmitted; evidence contains only the scanner's mask.
        from orchestral_worker.client import OrchestralClient
        client = OrchestralClient(base_url=CORE_URL, api_key=TOKEN)
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
            client.submit_finding(payload)
            submitted += 1

        with _jobs_lock:
            _jobs[job_id].update({
                "status": "cancelled" if result.get("status") == "cancelled" else "completed",
                "workspace": relative,
                "scanned_files_findings": len(findings),
                "findings_submitted": submitted,
                "findings": findings,
            })
    except Exception as exc:  # surfaced as a bounded job error
        with _jobs_lock:
            _jobs[job_id].update({"status": "failed", "error": f"{type(exc).__name__}: {exc}"})
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
        raise SystemExit("ORCHESTRAL_ADMIN_KEY must be configured before starting the worker")
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
