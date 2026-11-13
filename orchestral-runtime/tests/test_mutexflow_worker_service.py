import os
import tempfile
import threading
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


if __name__ == "__main__":
    unittest.main()
