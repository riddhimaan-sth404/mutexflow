#!/usr/bin/env python3
"""
Orchestral End-to-End Terminal Tool & MCP Integration Test Suite.
Validates:
1. Orchestral Citadel SQLite WAL citadel connectivity
2. Hardened Tartarus Kali container execution (Nmap, curl)
3. PerimeterLock deterministic scope boundary enforcement and token inspection
4. IronGate synchronous risk classification and approval gates
5. Model Context Protocol (MCP) JSON-RPC 2.0 stdio handshake & tool execution
6. Agent-token target scope, identity, and tool enforcement
7. Canonical Goose Operative Recipes validity

The suite expects a running core on 127.0.0.1:8088. The admin key is read
from ORCHESTRAL_API_KEY, with a test-only fallback for local runs.
"""

import base64
import json
import os
import subprocess
import sys
import tempfile
import unittest
import urllib.error
import urllib.request
from pathlib import Path

SWARM_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(SWARM_ROOT / "orchestral-worker"))

from orchestral_worker.client import OrchestralClient
from orchestral_worker.guard.scope import ScopeGuard, ScopeViolationError
from orchestral_worker.guard.hitl import HITLInterceptor, RiskTier
from orchestral_worker.sandbox.kali import KaliSandboxManager
from orchestral_worker.profiles import list_profiles

TEST_ADMIN_KEY = "asw_live_testadminsecret1234567890abcdef"


class TestOrchestralTerminalAndMCP(unittest.TestCase):

    @classmethod
    def setUpClass(cls):
        cls.core_url = os.environ.get("ORCHESTRAL_CORE_URL", "http://127.0.0.1:8088")
        cls.admin_key = os.environ.get("ORCHESTRAL_API_KEY", TEST_ADMIN_KEY)
        cls.client = OrchestralClient(base_url=cls.core_url, api_key=cls.admin_key)
        cls.scope = ScopeGuard(allowlist=["127.0.0.1/32", "*.internal.local"])
        cls.hitl = HITLInterceptor(auto_approve_tier=RiskTier.ACTIVE, client=cls.client)

    def test_01_citadel_connectivity_and_wal(self):
        """Verify Orchestral Citadel is up with SQLite WAL engine."""
        health = self.client.get_health()
        self.assertEqual(health.get("status"), "ok")
        self.assertEqual(health.get("storage"), "sqlite-wal")

    def test_02_tartarus_terminal_execution(self):
        """Verify KaliSandboxManager executes terminal tools inside Tartarus container."""
        mgr = KaliSandboxManager(agent_id="e2e-test", scope_guard=self.scope, hitl_interceptor=self.hitl)
        try:
            mgr.start_sandbox()
            if not mgr._is_running:
                self.skipTest("No container runtime/image available for Tartarus execution")

            # Nmap execution (exit code 0 even when no ports are visible)
            res_nmap = mgr.execute("nmap -sT -Pn -n -p 8088 127.0.0.1", "127.0.0.1")
            self.assertEqual(res_nmap.get("exit_code"), 0, res_nmap)
            self.assertIn("Nmap", res_nmap.get("stdout", ""))
            self.assertEqual(res_nmap.get("sandbox"), "hardened_podman_container")

            # Curl execution
            res_curl = mgr.execute("curl --version", "127.0.0.1")
            self.assertEqual(res_curl.get("exit_code"), 0)
            self.assertIn("curl", res_curl.get("stdout", "").lower())
        finally:
            mgr.stop_sandbox()

    def test_03_perimeter_lock_scope_enforcement(self):
        """Verify PerimeterLock blocks explicit out-of-scope targets and argument smuggling."""
        mgr = KaliSandboxManager(agent_id="e2e-scope", scope_guard=self.scope, hitl_interceptor=self.hitl)

        # 1. Out of scope explicit target
        res_explicit = mgr.execute("nmap 9.9.9.9", "9.9.9.9")
        self.assertEqual(res_explicit.get("status"), "scope_violation")
        self.assertIn("NOT in authorized CIDR list", res_explicit.get("error", ""))

        # 2. Scope smuggling attempt in command arguments
        res_smuggle = mgr.execute("nmap -sS 9.9.9.9", "127.0.0.1")
        self.assertEqual(res_smuggle.get("status"), "scope_violation")
        self.assertIn("NOT in authorized CIDR list", res_smuggle.get("error", ""))

        # 3. Shell substitution cannot be statically scoped
        res_subst = mgr.execute("curl http://$(cat /etc/hostname)", "127.0.0.1")
        self.assertEqual(res_subst.get("status"), "scope_violation")

    def test_04_irongate_risk_classification(self):
        """Verify IronGate differentiates passive inspection from active attacks."""
        mgr = KaliSandboxManager(agent_id="e2e-hitl", scope_guard=self.scope, hitl_interceptor=self.hitl)

        # Active network probes
        self.assertEqual(mgr.classify_risk_tier("nmap -sV -p 80 127.0.0.1"), RiskTier.ACTIVE)
        self.assertEqual(mgr.classify_risk_tier("curl -sI http://127.0.0.1:8088"), RiskTier.ACTIVE)
        self.assertEqual(mgr.classify_risk_tier("gobuster dir -u http://127.0.0.1 -w words.txt"), RiskTier.ACTIVE)

        # High Impact
        self.assertEqual(mgr.classify_risk_tier("john --wordlist=rockyou.txt hashes.txt"), RiskTier.HIGH_IMPACT)
        self.assertEqual(mgr.classify_risk_tier("sqlmap -u http://127.0.0.1/item?id=1 --batch"), RiskTier.HIGH_IMPACT)
        self.assertEqual(mgr.classify_risk_tier("rm -rf /tmp/evidence"), RiskTier.HIGH_IMPACT)

        # Passive local inspection
        self.assertEqual(mgr.classify_risk_tier("ls -la /workspace"), RiskTier.PASSIVE)

    def test_05_mcp_server_stdio_handshake(self):
        """Verify Orchestral MCP server initializes and lists all 5 tools over stdio."""
        env = os.environ.copy()
        env["ORCHESTRAL_API_KEY"] = self.admin_key
        proc = subprocess.Popen(
            ["python3", "-m", "orchestral_worker.mcp.server"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            env=env,
        )
        try:
            # Initialize
            proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}) + "\n")
            proc.stdin.flush()
            init_resp = json.loads(proc.stdout.readline())
            self.assertEqual(init_resp.get("result", {}).get("serverInfo", {}).get("name"), "orchestral-security-mcp")

            # Tools list
            proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}) + "\n")
            proc.stdin.flush()
            list_resp = json.loads(proc.stdout.readline())
            tools = [t["name"] for t in list_resp.get("result", {}).get("tools", [])]
            for name in ("tartarus_exec", "secret_scan", "codeql_analyze", "nexus_record_service", "nexus_query_state"):
                self.assertIn(name, tools)

            # Secret scan over MCP
            with tempfile.TemporaryDirectory() as tmp:
                secret_file = Path(tmp) / ".env"
                secret_file.write_text("AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE\n", encoding="utf-8")
                call_msg = {
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "tools/call",
                    "params": {
                        "name": "secret_scan",
                        "arguments": {"target": tmp, "directory_path": tmp},
                    },
                }
                proc.stdin.write(json.dumps(call_msg) + "\n")
                proc.stdin.flush()
                call_resp = json.loads(proc.stdout.readline())
                call_content = json.loads(call_resp["result"]["content"][0]["text"])
                self.assertGreaterEqual(call_content.get("findings_count", 0), 1)

            # Nexus query over MCP
            proc.stdin.write(json.dumps({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {"name": "nexus_query_state", "arguments": {}},
            }) + "\n")
            proc.stdin.flush()
            nexus_resp = json.loads(proc.stdout.readline())
            nexus_content = json.loads(nexus_resp["result"]["content"][0]["text"])
            self.assertIn("services", nexus_content)
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=5)
            for stream in (proc.stdin, proc.stdout, proc.stderr):
                try:
                    stream.close()
                except Exception:
                    pass

    def test_06_agent_token_scope_enforcement(self):
        """Agent tokens may only act on their issued target, identity, and tools."""
        issued = self.client.create_agent_token(
            agent_id="test-scope-agent",
            role="recon",
            target_scope="127.0.0.1",
            allowed_tools=["native_http_probe"],
            valid_seconds=300,
        )
        token = issued["token"]

        def post(path, payload, key=token):
            req = urllib.request.Request(
                f"{self.core_url}{path}",
                data=json.dumps(payload).encode("utf-8"),
                headers={"X-Orchestral-API-Key": key, "Content-Type": "application/json"},
                method="POST",
            )
            try:
                with urllib.request.urlopen(req, timeout=10) as resp:
                    return resp.status
            except urllib.error.HTTPError as e:
                return e.code

        self.assertEqual(
            post("/api/v1/findings", {
                "agent_id": "test-scope-agent",
                "target": "127.0.0.1",
                "title": "in-scope",
                "severity": "LOW",
                "description": "ok",
            }),
            201,
        )
        self.assertEqual(
            post("/api/v1/findings", {
                "agent_id": "test-scope-agent",
                "target": "evil.example.com",
                "title": "out-of-scope",
                "severity": "LOW",
                "description": "bad",
            }),
            403,
        )
        self.assertEqual(
            post("/api/v1/findings", {
                "agent_id": "someone-else",
                "target": "127.0.0.1",
                "title": "impersonation",
                "severity": "LOW",
                "description": "bad",
            }),
            403,
        )
        self.assertEqual(
            post("/api/v1/findings", {
                "agent_id": "test-scope-agent",
                "target": "127.0.0.1",
                "title": "self-confirmed",
                "severity": "HIGH",
                "description": "bad",
                "status": "confirmed",
            }),
            403,
        )
        self.assertEqual(
            post("/api/v1/hitl/pending", {
                "agent_id": "test-scope-agent",
                "tool_name": "sqlmap",
                "target": "127.0.0.1",
                "command_preview": "sqlmap",
                "risk_tier": "high_impact",
            }),
            403,
        )

        # Revoke and verify the token is rejected.
        payload = token[4:].split(".")[1]
        payload += "=" * (-len(payload) % 4)
        jti = json.loads(base64.urlsafe_b64decode(payload))["jti"]
        self.client._request(f"/api/v1/auth/agent-token/{jti}/revoke", method="POST")

        req = urllib.request.Request(
            f"{self.core_url}/api/v1/findings",
            headers={"X-Orchestral-API-Key": token},
        )
        with self.assertRaises(urllib.error.HTTPError) as ctx:
            urllib.request.urlopen(req, timeout=10)
        self.assertEqual(ctx.exception.code, 401)

    def test_07_canonical_goose_recipes(self):
        """Verify all operative recipes validate cleanly."""
        recipes = list_profiles()
        self.assertGreaterEqual(len(recipes), 5)
        recipe_keys = [r.key for r in recipes]
        self.assertIn("sentinel_recon", recipe_keys)
        self.assertIn("cerberus_dast", recipe_keys)
        self.assertIn("athena_sast", recipe_keys)
        self.assertIn("justiciar_triage", recipe_keys)
        self.assertIn("archon_coordinator", recipe_keys)

        goose_bin = SWARM_ROOT / "bin" / "goose"
        if not goose_bin.exists():
            self.skipTest("Goose binary not available")
        for r in recipes:
            recipe_file = SWARM_ROOT / "recipes" / r.key / "recipe.yaml"
            if not recipe_file.exists():
                continue
            res = subprocess.run([str(goose_bin), "recipe", "validate", str(recipe_file)], capture_output=True, text=True)
            self.assertEqual(res.returncode, 0, f"Recipe validation failed for {r.key}: {res.stderr}")


if __name__ == "__main__":
    unittest.main(verbosity=2)
