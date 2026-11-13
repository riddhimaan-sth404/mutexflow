#!/usr/bin/env python3
"""
Orchestral Model Context Protocol (MCP) Server.
Bridges Orchestral Desktop & CLI to security capabilities:
- Tartarus hardened Kali Linux container (Nmap, JtR, Nikto, Trufflehog, curl)
- PerimeterLock deterministic ScopeGuard
- IronGate zero-trust Human-in-the-Loop approval gate
- SynapseNexus collaborative intelligence blackboard
"""

import contextlib
import sys
import json
import os
from typing import Dict, Any, List

from orchestral_worker.client import OrchestralClient
from orchestral_worker.guard.scope import ScopeGuard, ScopeViolationError
from orchestral_worker.guard.hitl import HITLInterceptor, RiskTier
from orchestral_worker.tools.registry import ToolRegistry


PROTOCOL_VERSION = "2024-11-05"


class OrchestralMCPServer:
    def __init__(self):
        core_url = os.environ.get("ORCHESTRAL_CORE_URL", "http://127.0.0.1:8088")
        # No hard-coded production credential. Operators must provide a key.
        api_key = os.environ.get("ORCHESTRAL_API_KEY", "")

        self.client = OrchestralClient(base_url=core_url, api_key=api_key)

        # PerimeterLock allowlist
        raw_allow = os.environ.get("ORCHESTRAL_SCOPE", "*.internal.local,127.0.0.1/32,10.0.0.0/8")
        self.scope_guard = ScopeGuard(allowlist=[s.strip() for s in raw_allow.split(",") if s.strip()])

        # IronGate HITL
        self.hitl = HITLInterceptor(auto_approve_tier=RiskTier.PASSIVE, client=self.client)

        # Arsenal tools
        self.tools = ToolRegistry(scope_guard=self.scope_guard, hitl=self.hitl)

    def get_tool_definitions(self) -> List[Dict[str, Any]]:
        return [
            {
                "name": "tartarus_exec",
                "description": (
                    "Execute terminal security tools (e.g. nmap, curl, john, nikto, semgrep, trufflehog) "
                    "inside an isolated, capability-dropped Kali Linux sandbox container. "
                    "Always prefer non-interactive flags (e.g. nmap -sV --open, curl -sI, john --show). "
                    "Commands are inspected by PerimeterLock for authorized target boundaries, "
                    "and active operations are automatically gated by IronGate for operator approval."
                ),
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "target": {"type": "string", "description": "Target hostname, IP, CIDR, or URL within authorized scope"},
                        "command": {"type": "string", "description": "Terminal command to execute (e.g., 'nmap -sV -p 80,443 --open 127.0.0.1' or 'curl -sI http://127.0.0.1')"},
                        "agent_id": {"type": "string", "description": "Identifier of the calling operative (default: goose-operative)"},
                    },
                    "required": ["target", "command"],
                },
            },
            {
                "name": "secret_scan",
                "description": "Scans local repository code for hardcoded secrets, cloud API keys, and private certificates.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "target": {"type": "string", "description": "Local directory path to scan (default: '.')"},
                        "directory_path": {"type": "string", "description": "Directory path override"},
                    },
                    "required": ["target"],
                },
            },
            {
                "name": "codeql_analyze",
                "description": "Executes semantic AST taint-tracking query analysis using CodeQL against a source directory.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "target": {"type": "string", "description": "Path to codebase repository"},
                        "language": {"type": "string", "description": "Language (python, javascript, go, java)"},
                    },
                    "required": ["target"],
                },
            },
            {
                "name": "nexus_record_service",
                "description": "Publishes a newly discovered service to the SynapseNexus shared swarm blackboard.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "target": {"type": "string", "description": "Host IP or domain"},
                        "port": {"type": "integer", "description": "Port number"},
                        "service_name": {"type": "string", "description": "Service name (e.g., http, ssh)"},
                        "banner": {"type": "string", "description": "Discovered banner text"},
                    },
                    "required": ["target", "port", "service_name"],
                },
            },
            {
                "name": "nexus_query_state",
                "description": "Queries current collaborative intelligence from SynapseNexus (services, endpoints, candidates, findings).",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                },
            },
        ]

    def handle_call_tool(self, name: str, args: Dict[str, Any]) -> Dict[str, Any]:
        target = args.get("target", "")

        try:
            if name == "tartarus_exec":
                tool = self.tools.get_tool("kali_terminal")
                cmd = args.get("command", "")
                agent_id = args.get("agent_id", "goose-operative")
                res = tool.execute(agent_id=agent_id, target=target, command=cmd)
                return {"content": [{"type": "text", "text": json.dumps(res, indent=2)}]}

            elif name == "secret_scan":
                tool = self.tools.get_tool("native_secret_scanner")
                directory = args.get("directory_path") or target or "."
                res = tool.execute(
                    agent_id="goose-operative",
                    target=target or directory,
                    directory_path=directory,
                )
                return {"content": [{"type": "text", "text": json.dumps(res, indent=2)}]}

            elif name == "codeql_analyze":
                tool = self.tools.get_tool("codeql_analyzer")
                lang = args.get("language") or "python"
                res = tool.execute(agent_id="goose-operative", target=target, language=lang)
                return {"content": [{"type": "text", "text": json.dumps(res, indent=2)}]}

            elif name == "nexus_record_service":
                port = int(args.get("port", 0))
                s_name = args.get("service_name", "unknown")
                banner = args.get("banner", "")
                res = self.client.record_service(
                    target=target,
                    port=port,
                    service_name=s_name,
                    banner=banner,
                    discovered_by="goose-mcp-operative",
                )
                return {"content": [{"type": "text", "text": json.dumps(res, indent=2)}]}

            elif name == "nexus_query_state":
                services = self.client.list_services()
                endpoints = self.client.list_endpoints()
                candidates = self.client.list_candidates()
                findings = self.client.list_findings()
                summary = {
                    "services": services,
                    "endpoints": endpoints,
                    "candidates": candidates,
                    "findings_count": len(findings),
                    "recent_findings": findings[:5],
                }
                return {"content": [{"type": "text", "text": json.dumps(summary, indent=2)}]}

            else:
                return {
                    "isError": True,
                    "content": [{"type": "text", "text": f"Unknown tool: '{name}'"}],
                }

        except ScopeViolationError as e:
            return {
                "isError": True,
                "content": [{"type": "text", "text": f"PerimeterLock Scope Violation: {str(e)}"}],
            }
        except Exception as e:
            return {
                "isError": True,
                "content": [{"type": "text", "text": f"Execution error: {str(e)}"}],
            }

    def _dispatch(self, req: Dict[str, Any]) -> Dict[str, Any]:
        msg_id = req.get("id")
        method = req.get("method")
        params = req.get("params", {}) or {}

        if method == "initialize":
            return {
                "jsonrpc": "2.0",
                "id": msg_id,
                "result": {
                    "protocolVersion": PROTOCOL_VERSION,
                    "serverInfo": {
                        "name": "orchestral-security-mcp",
                        "version": "0.3.0",
                    },
                    "capabilities": {
                        "tools": {},
                        "resources": {},
                        "prompts": {},
                    },
                },
            }
        if method == "ping":
            return {"jsonrpc": "2.0", "id": msg_id, "result": {}}
        if method == "tools/list":
            return {
                "jsonrpc": "2.0",
                "id": msg_id,
                "result": {"tools": self.get_tool_definitions()},
            }
        if method == "tools/call":
            name = params.get("name")
            args = params.get("arguments", {}) or {}
            # Any tool that prints to stdout would corrupt the JSON-RPC stream.
            with contextlib.redirect_stdout(sys.stderr):
                res = self.handle_call_tool(name, args)
            return {"jsonrpc": "2.0", "id": msg_id, "result": res}
        if method == "resources/list":
            return {"jsonrpc": "2.0", "id": msg_id, "result": {"resources": []}}
        if method == "resources/templates/list":
            return {"jsonrpc": "2.0", "id": msg_id, "result": {"resourceTemplates": []}}
        if method == "prompts/list":
            return {"jsonrpc": "2.0", "id": msg_id, "result": {"prompts": []}}
        if method == "logging/setLevel":
            return {"jsonrpc": "2.0", "id": msg_id, "result": {}}
        if method in ("notifications/initialized", "notifications/cancelled"):
            return None  # notifications never get a response

        if msg_id is None:
            # Unknown notification: ignore rather than emitting id:null errors.
            return None

        return {
            "jsonrpc": "2.0",
            "id": msg_id,
            "error": {
                "code": -32601,
                "message": f"Method not found: {method}",
            },
        }

    def run_stdio(self):
        """Standard JSON-RPC 2.0 loop reading from stdin and writing to stdout."""
        for line in sys.stdin:
            line = line.strip()
            if not line:
                continue

            try:
                req = json.loads(line)
            except json.JSONDecodeError:
                continue

            resp = self._dispatch(req)
            if resp is None:
                continue

            sys.stdout.write(json.dumps(resp) + "\n")
            sys.stdout.flush()


def main():
    """Entry point used by `orchestral mcp` and the Goose extension config."""
    server = OrchestralMCPServer()
    server.run_stdio()


if __name__ == "__main__":
    main()
