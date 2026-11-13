#!/usr/bin/env python3
"""Unit tests for the guardrails and tool plumbing.

These tests do not require the core daemon or a container runtime.
"""

import sys
import unittest
from pathlib import Path
from typing import Any, Dict

SWARM_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(SWARM_ROOT / "orchestral-worker"))

from orchestral_worker.guard.hitl import HITLInterceptor, RiskTier
from orchestral_worker.guard.scope import ScopeGuard, ScopeViolationError
from orchestral_worker.tools.base import BaseSecurityTool
from orchestral_worker.tools.registry import ToolRegistry
from orchestral_worker.tools.secret_scanner import NativeSecretScannerTool


class TestScopeGuard(unittest.TestCase):
    def test_bare_ip_allowlist(self):
        guard = ScopeGuard(allowlist=["127.0.0.1"])
        self.assertEqual(guard.allowlist, ["127.0.0.1/32"])
        self.assertTrue(guard.validate_target("127.0.0.1"))
        self.assertTrue(guard.validate_target("http://127.0.0.1:8088/path"))

    def test_out_of_scope_domain(self):
        guard = ScopeGuard(allowlist=["127.0.0.1"])
        with self.assertRaises(ScopeViolationError):
            guard.validate_target("evil.example.com")

    def test_private_range_detection_is_not_substring(self):
        # "210.0.0.0/8" contains "10." but is public; it must not implicitly
        # enable private addressing.
        guard = ScopeGuard(allowlist=["210.0.0.0/8"])
        with self.assertRaises(ScopeViolationError):
            guard.validate_target("10.1.2.3")
        self.assertTrue(guard.validate_target("210.1.2.3"))

    def test_explicit_private_cidr_is_allowed(self):
        guard = ScopeGuard(allowlist=["10.0.0.0/8"])
        self.assertTrue(guard.validate_target("10.20.30.40"))

    def test_wildcard_domain(self):
        guard = ScopeGuard(allowlist=["*.internal.local"])
        self.assertTrue(guard.validate_target("api.internal.local"))
        self.assertTrue(guard.validate_target("https://api.internal.local:8443/x"))
        with self.assertRaises(ScopeViolationError):
            guard.validate_target("internal.local.evil.com")

    def test_trailing_dot_and_ipv6(self):
        guard = ScopeGuard(allowlist=["example.com", "::1/128"])
        self.assertTrue(guard.validate_target("example.com."))
        self.assertTrue(guard.validate_target("[::1]:8088"))

    def test_metadata_endpoint_denied(self):
        guard = ScopeGuard(allowlist=["169.254.169.254/32"])
        with self.assertRaises(ScopeViolationError):
            guard.validate_target("169.254.169.254")

    def test_local_paths_allowed(self):
        guard = ScopeGuard(allowlist=["127.0.0.1"])
        self.assertTrue(guard.validate_target("./src"))
        self.assertTrue(guard.validate_target("/tmp/repo"))


class TestHITLClassification(unittest.TestCase):
    def setUp(self):
        self.hitl = HITLInterceptor()

    def test_exact_tool_names(self):
        self.assertEqual(self.hitl.classify_tool("native_secret_scanner"), RiskTier.PASSIVE)
        self.assertEqual(self.hitl.classify_tool("native_http_probe"), RiskTier.ACTIVE)
        self.assertEqual(self.hitl.classify_tool("sqlmap"), RiskTier.HIGH_IMPACT)
        self.assertIsNone(self.hitl.classify_tool("my-custom-scanner"))

    def test_word_boundaries(self):
        # "scurvy" must not match the "curl" pattern.
        self.assertEqual(
            self.hitl.evaluate_risk("scurvy --help", "my-custom-scanner"),
            RiskTier.PASSIVE,
        )
        self.assertEqual(
            self.hitl.evaluate_risk("curl -w '%{http_code}' http://127.0.0.1", "kali:curl"),
            RiskTier.ACTIVE,
        )

    def test_high_impact_patterns(self):
        self.assertEqual(
            self.hitl.evaluate_risk("sqlmap -u http://127.0.0.1/?id=1", "kali:sqlmap"),
            RiskTier.HIGH_IMPACT,
        )
        self.assertEqual(
            self.hitl.evaluate_risk("rm -rf /tmp/x", "kali:rm"),
            RiskTier.HIGH_IMPACT,
        )

    def test_explicit_tier_wins(self):
        self.assertEqual(
            self.hitl.evaluate_risk("nmap 127.0.0.1", "native_port_scanner", risk_tier=RiskTier.HIGH_IMPACT),
            RiskTier.HIGH_IMPACT,
        )

    def test_auto_approval_threshold(self):
        passive = HITLInterceptor(auto_approve_tier=RiskTier.PASSIVE, approval_callback=lambda *_: False)
        self.assertTrue(passive.request_approval("a", "native_secret_scanner", ".", "scan"))
        self.assertFalse(passive.request_approval("a", "native_http_probe", "127.0.0.1", "probe"))

        active = HITLInterceptor(auto_approve_tier=RiskTier.ACTIVE, approval_callback=lambda *_: False)
        self.assertTrue(active.request_approval("a", "native_http_probe", "127.0.0.1", "probe"))
        self.assertFalse(active.request_approval("a", "sqlmap", "127.0.0.1", "sqlmap"))


class _FailingTool(BaseSecurityTool):
    def __init__(self, **kwargs):
        super().__init__(name="failing_tool", description="always fails", risk_tier=RiskTier.PASSIVE, **kwargs)

    def _run(self, target: str, **kwargs) -> Dict[str, Any]:
        raise RuntimeError("boom")


class TestToolExecution(unittest.TestCase):
    def test_exception_becomes_error_result(self):
        tool = _FailingTool()
        result = tool.execute(agent_id="a", target=".")
        self.assertEqual(result["status"], "error")
        self.assertIn("RuntimeError: boom", result["error"])

    def test_registry_role_mapping(self):
        registry = ToolRegistry()
        self.assertIn("native_port_scanner", registry.get_tools_for_role("sentinel_recon"))
        self.assertIn("native_http_probe", registry.get_tools_for_role("cerberus_dast"))
        self.assertIn("codeql_analyzer", registry.get_tools_for_role("athena_sast"))
        self.assertIn("native_secret_scanner", registry.get_tools_for_role("justiciar_triage"))


class TestSecretScannerReadGuard(unittest.TestCase):
    def setUp(self):
        self.scanner = NativeSecretScannerTool()

    def test_denies_system_and_credential_paths(self):
        for path in ("/etc", "/proc", "/sys", "/root", "~/.ssh", "~/.aws", "~/.gnupg"):
            result = self.scanner.run(agent_id="a", target=path, directory_path=path)
            self.assertIn(result.get("status"), ("denied", "error"), path)
            self.assertEqual(result.get("findings_count"), 0, path)
            self.assertEqual(result.get("findings"), [], path)

    def test_scans_regular_workspace_directory(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            secret = Path(tmp) / "config.py"
            secret.write_text("AWS_ACCESS_KEY_ID = 'AKIAIOSFODNN7EXAMPLE'\n", encoding="utf-8")
            result = self.scanner.run(agent_id="a", target=tmp, directory_path=tmp)
            self.assertEqual(result.get("findings_count"), 1, result)
            self.assertEqual(result["findings"][0]["type"], "AWS Access Key")

    def test_does_not_follow_symlink_escape(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp, tempfile.TemporaryDirectory() as outside_dir:
            outside = Path(outside_dir) / "outside-secret.txt"
            outside.write_text("AKIAIOSFODNN7EXAMPLE\n", encoding="utf-8")
            scan_dir = Path(tmp) / "project"
            scan_dir.mkdir()
            (scan_dir / "link.py").symlink_to(outside)
            result = self.scanner.run(agent_id="a", target=str(scan_dir), directory_path=str(scan_dir))
            self.assertEqual(result.get("findings_count"), 0, result)


if __name__ == "__main__":
    unittest.main(verbosity=2)
