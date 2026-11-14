import os
import tempfile
import threading
import time
import unittest
from pathlib import Path

from orchestral_worker import service
from orchestral_worker.tools.secret_scanner import NativeSecretScannerTool


class LocalWorkspaceBoundaryTests(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.root = Path(self.temp_dir.name).resolve()
        self.original_root = service.WORKSPACE_ROOT
        service.WORKSPACE_ROOT = self.root
        (self.root / "project").mkdir()

    def tearDown(self):
        service.WORKSPACE_ROOT = self.original_root
        self.temp_dir.cleanup()

    def test_accepts_workspace_relative_to_root(self):
        self.assertEqual(service._resolve_workspace("project"), self.root / "project")

    def test_rejects_absolute_workspace_path(self):
        with self.assertRaises(ValueError):
            service._resolve_workspace(str(self.root / "project"))

    def test_rejects_parent_traversal(self):
        with self.assertRaises(ValueError):
            service._resolve_workspace("../outside")

    @unittest.skipUnless(os.name == "nt", "Windows drive-qualified path test")
    def test_rejects_drive_qualified_relative_path(self):
        with self.assertRaises(ValueError):
            service._resolve_workspace("C:outside")

    def test_rejects_symlink_escape(self):
        outside = Path(self.temp_dir.name).parent / (Path(self.temp_dir.name).name + "-outside")
        outside.mkdir(exist_ok=True)
        link = self.root / "escape"
        try:
            link.symlink_to(outside, target_is_directory=True)
        except (OSError, NotImplementedError):
            self.skipTest("symlink creation is unavailable on this platform")
        try:
            with self.assertRaises(ValueError):
                service._resolve_workspace("escape")
        finally:
            try:
                link.unlink()
                outside.rmdir()
            except OSError:
                pass

    def test_local_scan_honors_cancellation(self):
        cancel_event = threading.Event()
        cancel_event.set()
        result = NativeSecretScannerTool().execute(
            agent_id="test-agent",
            target=str(self.root / "project"),
            directory_path=str(self.root / "project"),
            cancel_event=cancel_event,
        )
        self.assertEqual(result.get("status"), "cancelled")

    def test_secret_scanner_masks_detected_value(self):
        secret = "ghp_" + ("a" * 36)
        findings = NativeSecretScannerTool()._scan_text(f"TOKEN={secret}", "sample.py")
        self.assertTrue(findings)
        self.assertNotIn(secret, findings[0]["preview"])



class SeverityMappingTests(unittest.TestCase):
    def test_high_confidence_provider_credentials_map_to_high(self):
        for pattern in ("AWS Access Key", "AWS Secret Key", "Generic Private Key", "GitHub Personal Token"):
            severity, cvss = service._severity_for(pattern)
            self.assertEqual(severity, "HIGH")
            self.assertGreaterEqual(cvss, 7.0)

    def test_scoped_or_short_lived_credentials_map_to_medium(self):
        for pattern in ("Google Cloud API Key", "Slack Webhook / Token", "JWT Token"):
            severity, cvss = service._severity_for(pattern)
            self.assertEqual(severity, "MEDIUM")
            self.assertLess(cvss, 7.0)

    def test_unknown_patterns_fall_back_to_low(self):
        severity, cvss = service._severity_for("Some New Pattern")
        self.assertEqual(severity, "LOW")
        self.assertLess(cvss, 5.0)


class JobRegistryTTLTests(unittest.TestCase):
    def tearDown(self):
        with service._jobs_lock:
            service._jobs.clear()
            service._cancel_events.clear()

    def test_finished_jobs_are_pruned_after_ttl(self):
        job_id = "11111111-2222-3333-4444-555555555555"
        with service._jobs_lock:
            service._jobs[job_id] = {
                "job_id": job_id,
                "status": "completed",
                "finished_at": time.monotonic() - service.JOB_TTL_SECONDS - 1,
            }
        service._prune_stale_jobs()
        self.assertNotIn(job_id, service._jobs)

    def test_active_jobs_are_never_pruned(self):
        job_id = "99999999-8888-7777-6666-555555555555"
        with service._jobs_lock:
            service._jobs[job_id] = {
                "job_id": job_id,
                "status": "running",
                "finished_at": time.monotonic() - service.JOB_TTL_SECONDS - 1,
            }
        service._prune_stale_jobs()
        self.assertIn(job_id, service._jobs)


if __name__ == "__main__":
    unittest.main()
