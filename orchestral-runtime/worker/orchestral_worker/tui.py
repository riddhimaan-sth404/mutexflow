import json
import os
import sys
import time
import argparse
import threading
import urllib.request
from datetime import datetime, timezone
from typing import Dict, Any, List, Optional

from rich.console import Console
from rich.layout import Layout
from rich.panel import Panel
from rich.table import Table
from rich.text import Text
from rich.live import Live

from orchestral_worker.client import OrchestralClient


class OpenCodeStyleTUI:
    """
    Terminal User Interface inspired by anomalyco/opencode:
    High-density split-pane layout for real-time AI agent swarm observability,
    security finding tracking, and Human-in-the-Loop decision controls.

    Connected via live SSE (Server-Sent Events) streaming from Orchestral Citadel for
    sub-millisecond reactivity.
    """

    def __init__(self, core_url: str, api_key: str):
        self.console = Console()
        self.client = OrchestralClient(base_url=core_url, api_key=api_key)
        self.api_key = api_key
        self.core_url = core_url
        self.layout = Layout()
        self.setup_layout()

        # Real-time state cache
        self._lock = threading.Lock()
        self.agents: List[Dict[str, Any]] = []
        self.findings: List[Dict[str, Any]] = []
        self.pending_hitl: List[Dict[str, Any]] = []
        self.blackboard_summary: Dict[str, Any] = {}
        self.system_logs: List[Dict[str, Any]] = []
        self.is_connected = False
        self.mode = "COORDINATED"
        self._stop = threading.Event()

        # Start live SSE subscriber
        self._start_sse_subscriber()

    def _start_sse_subscriber(self):
        """Background thread connecting to Orchestral Citadel SSE event stream."""
        def sse_worker():
            sse_url = f"{self.core_url}/api/v1/events/sse"
            while not self._stop.is_set():
                try:
                    req = urllib.request.Request(
                        sse_url,
                        headers={
                            "Accept": "text/event-stream",
                            "X-Orchestral-API-Key": self.api_key,
                        },
                    )
                    with urllib.request.urlopen(req, timeout=30) as resp:
                        self.is_connected = True
                        current_event = None
                        for line in resp:
                            if self._stop.is_set():
                                break
                            line_str = line.decode("utf-8").strip()
                            if not line_str:
                                continue
                            if line_str.startswith("event:"):
                                current_event = line_str.split(":", 1)[1].strip()
                            elif line_str.startswith("data:"):
                                raw_data = line_str.split(":", 1)[1].strip()
                                try:
                                    payload = json.loads(raw_data)
                                    self._handle_live_event(current_event, payload)
                                except Exception:
                                    pass
                except Exception:
                    self.is_connected = False
                    self._stop.wait(3)

        t = threading.Thread(target=sse_worker, daemon=True)
        t.start()

    def _handle_live_event(self, event_type: Optional[str], data: Any):
        with self._lock:
            if event_type == "agent.updated":
                self.agents = [a for a in self.agents if a.get("id") != data.get("id")]
                self.agents.append(data)
            elif event_type == "finding.reported":
                self.findings = [f for f in self.findings if f.get("id") != data.get("id")]
                self.findings.append(data)
            elif event_type == "hitl.pending":
                self.pending_hitl = [h for h in self.pending_hitl if h.get("id") != data.get("id")]
                self.pending_hitl.append(data)
            elif event_type == "hitl.resolved":
                res_id = data.get("id")
                self.pending_hitl = [h for h in self.pending_hitl if str(h.get("id")) != str(res_id)]
            elif event_type == "blackboard.updated":
                self.blackboard_summary = data if isinstance(data, dict) else {}
            elif event_type == "system.log":
                self.system_logs.append(data)
                self.system_logs = self.system_logs[-50:]

    def _start_keyboard_listener(self):
        """Read single-key commands when attached to a TTY."""
        if not sys.stdin.isatty():
            return

        try:
            import select
            import termios
            import tty
        except ImportError:
            return

        def worker():
            fd = sys.stdin.fileno()
            try:
                old = termios.tcgetattr(fd)
            except Exception:
                return
            try:
                tty.setcbreak(fd)
                while not self._stop.is_set():
                    ready, _, _ = select.select([sys.stdin], [], [], 0.2)
                    if not ready:
                        continue
                    key = sys.stdin.read(1).lower()
                    if key == "q":
                        self._stop.set()
                        break
                    elif key == "c":
                        self.mode = "COORDINATED"
                    elif key == "i":
                        self.mode = "INDEPENDENT"
                    elif key == "a":
                        self._resolve_oldest("approve")
                    elif key == "d":
                        self._resolve_oldest("deny")
                    elif key == " ":
                        self.render_snapshot()
            finally:
                try:
                    termios.tcsetattr(fd, termios.TCSADRAIN, old)
                except Exception:
                    pass

        threading.Thread(target=worker, daemon=True).start()

    def _resolve_oldest(self, action: str):
        with self._lock:
            if not self.pending_hitl:
                return
            req_id = self.pending_hitl[0].get("id")
        try:
            if action == "approve":
                self.client.approve_hitl(str(req_id))
            else:
                self.client.deny_hitl(str(req_id))
            with self._lock:
                self.pending_hitl = [h for h in self.pending_hitl if str(h.get("id")) != str(req_id)]
        except Exception as e:
            self.system_logs.append({"level": "error", "message": str(e)})

    def setup_layout(self):
        self.layout.split(
            Layout(name="header", size=3),
            Layout(name="body", ratio=1),
            Layout(name="footer", size=3),
        )
        self.layout["body"].split_row(
            Layout(name="left_stream", ratio=6),
            Layout(name="right_roster", ratio=4),
        )
        self.layout["left_stream"].split(
            Layout(name="findings_feed", ratio=6),
            Layout(name="hitl_box", ratio=4),
        )
        self.layout["right_roster"].split(
            Layout(name="agents_table", ratio=6),
            Layout(name="metrics_box", ratio=4),
        )

    def render_header(self, mode: str = "COORDINATED") -> Panel:
        key_masked = f"{self.api_key[:12]}..." if len(self.api_key) > 14 else self.api_key
        header_text = Text()
        header_text.append("[ORCHESTRAL] ", style="bold cyan")
        header_text.append("| ", style="dim")
        status_dot = "[LIVE SSE] " if self.is_connected else "[POLLING] "
        status_style = "bold green" if self.is_connected else "bold yellow"
        header_text.append(status_dot, style=status_style)
        header_text.append(f"[{self.core_url}] ", style="dim")
        header_text.append("| ", style="dim")
        header_text.append("MODE: ", style="dim")
        mode_style = "bold magenta" if mode == "COORDINATED" else "bold yellow"
        header_text.append(f"[{mode}] ", style=mode_style)
        header_text.append("| ", style="dim")
        header_text.append("KEY: ", style="dim")
        header_text.append(key_masked, style="cyan")

        return Panel(
            header_text,
            style="bold white on #1a1b26",
            border_style="bright_blue",
        )

    def render_agents_table(self, agents: List[Dict[str, Any]]) -> Panel:
        table = Table(box=None, expand=True, pad_edge=False)
        table.add_column("Agent", style="bold white")
        table.add_column("Role", style="dim")
        table.add_column("Status", justify="right")

        status_styles = {
            "idle": "[dim]idle[/dim]",
            "thinking": "[bold cyan][thinking][/bold cyan]",
            "running_tool": "[bold yellow][running][/bold yellow]",
            "awaiting_approval": "[bold red blink][AWAIT HITL][/bold red blink]",
            "completed": "[bold green][DONE][/bold green]",
            "cancelled": "[bold yellow][CANCELLED][/bold yellow]",
            "failed": "[bold red][FAILED][/bold red]",
        }

        for a in agents:
            status_str = status_styles.get(a.get("status", "idle"), a.get("status"))
            table.add_row(
                a.get("id", "agent"),
                a.get("role", "specialist"),
                status_str,
            )

        if not agents:
            table.add_row("[dim]No agents registered[/dim]", "-", "-")

        return Panel(
            table,
            title="[bold cyan][AGENTS] Operative Swarm Roster[/bold cyan]",
            border_style="blue",
        )

    def render_findings_table(self, findings: List[Dict[str, Any]]) -> Panel:
        table = Table(box=None, expand=True, pad_edge=False)
        table.add_column("Sev", width=8)
        table.add_column("Target", style="cyan")
        table.add_column("Finding Title", style="bold white")
        table.add_column("CVSS", justify="right", width=6)

        sev_styles = {
            "CRITICAL": "[bold white on red] CRIT [/bold white on red]",
            "HIGH": "[bold red] HIGH [/bold red]",
            "MEDIUM": "[bold yellow] MED  [/bold yellow]",
            "LOW": "[bold blue] LOW  [/bold blue]",
            "INFO": "[dim] INFO [/dim]",
        }

        for f in reversed(findings[-8:]):
            sev_badge = sev_styles.get(f.get("severity", "INFO"), f.get("severity"))
            cvss_str = str(f.get("cvss")) if f.get("cvss") is not None else "-"
            table.add_row(
                sev_badge,
                f.get("target", "-")[:20],
                f.get("title", "Unknown")[:34],
                cvss_str,
            )

        if not findings:
            table.add_row("-", "[dim]No findings yet[/dim]", "[dim]Swarm analyzing attack surface...[/dim]", "-")

        return Panel(
            table,
            title="[bold red][FINDINGS] Live Vulnerability Intelligence (SARIF / CVSS v3.1)[/bold red]",
            border_style="red",
        )

    def render_hitl_box(self, pending: List[Dict[str, Any]]) -> Panel:
        if not pending:
            return Panel(
                Text("[OK] All autonomous agent actions within authorized risk boundaries.\n  No human-in-the-loop approvals pending.", style="dim green"),
                title="[bold green][GATE] IronGate: Human-In-The-Loop Authorization[/bold green]",
                border_style="green",
            )

        table = Table(box=None, expand=True, pad_edge=False)
        table.add_column("Req ID", style="bold white", width=12)
        table.add_column("Agent", style="cyan")
        table.add_column("Tool", style="yellow")
        table.add_column("Command Preview", style="bold red")

        for req in pending[:3]:
            req_id_short = str(req.get("id", ""))[:8]
            cmd = req.get("command_preview", "")
            cmd_preview = (cmd[:28] + "...") if len(cmd) > 31 else cmd
            table.add_row(
                req_id_short,
                req.get("agent_id", "-"),
                req.get("tool_name", "-"),
                cmd_preview,
            )

        return Panel(
            table,
            title="[bold red blink][ALERT] ACTION REQUIRED: Pending Operator Sign-Off[/bold red blink]",
            border_style="bright_red",
        )

    def render_metrics(self, agents: List[Dict[str, Any]], findings: List[Dict[str, Any]]) -> Panel:
        total_tokens = sum(a.get("tokens_used", 0) for a in agents)
        total_cost = sum(a.get("cost_usd", 0.0) for a in agents)
        crit_count = sum(1 for f in findings if f.get("severity") == "CRITICAL")
        high_count = sum(1 for f in findings if f.get("severity") == "HIGH")

        table = Table(box=None, expand=True)
        table.add_column("Metric", style="dim")
        table.add_column("Value", style="bold white", justify="right")

        table.add_row("Total Active Agents", str(len(agents)))
        table.add_row("Critical / High Findings", f"[bold red]{crit_count}[/bold red] / [red]{high_count}[/red]")
        table.add_row("Total Findings", str(len(findings)))
        table.add_row("Token Usage", f"{total_tokens:,}")
        table.add_row("Estimated API Cost", f"${total_cost:.4f}")
        table.add_row("Persistence Engine", "[green]SQLite WAL[/green]")

        return Panel(
            table,
            title="[bold yellow][METRICS] Swarm Telemetry & Token Budget[/bold yellow]",
            border_style="yellow",
        )

    def render_footer(self) -> Panel:
        footer_text = Text()
        footer_text.append("[Q] ", style="bold white")
        footer_text.append("Quit  ", style="dim")
        footer_text.append("[C] ", style="bold magenta")
        footer_text.append("Coordinated  ", style="dim")
        footer_text.append("[I] ", style="bold yellow")
        footer_text.append("Independent  ", style="dim")
        footer_text.append("[A] ", style="bold green")
        footer_text.append("Approve HITL  ", style="dim")
        footer_text.append("[D] ", style="bold red")
        footer_text.append("Deny HITL  ", style="dim")
        footer_text.append("[SPACE] ", style="bold white")
        footer_text.append("Refresh", style="dim")

        return Panel(
            footer_text,
            style="bold white on #1a1b26",
            border_style="dim",
        )

    def render_snapshot(self) -> Layout:
        # Pull latest from Core REST and update the cache.
        try:
            agents = self.client.list_agents()
            with self._lock:
                self.agents = agents
        except Exception:
            agents = self.agents

        try:
            findings = self.client.list_findings()
            with self._lock:
                self.findings = findings
        except Exception:
            findings = self.findings

        try:
            pending_hitl = self.client.list_pending_hitl()
            with self._lock:
                self.pending_hitl = pending_hitl
        except Exception:
            pending_hitl = self.pending_hitl

        self.layout["header"].update(self.render_header(self.mode))
        self.layout["findings_feed"].update(self.render_findings_table(findings))
        self.layout["hitl_box"].update(self.render_hitl_box(pending_hitl))
        self.layout["agents_table"].update(self.render_agents_table(agents))
        self.layout["metrics_box"].update(self.render_metrics(agents, findings))
        self.layout["footer"].update(self.render_footer())

        return self.layout

    def run_live(self, duration_seconds: int = 10, refresh_rate: float = 1.0):
        """Runs the live terminal monitor for a given duration or until interrupted."""
        self._stop.clear()
        self._start_keyboard_listener()
        with Live(self.render_snapshot(), console=self.console, screen=True, refresh_per_second=2) as live:
            start_time = time.time()
            while not self._stop.is_set():
                live.update(self.render_snapshot())
                time.sleep(refresh_rate)
                if duration_seconds > 0 and (time.time() - start_time) >= duration_seconds:
                    break

    def stop(self):
        self._stop.set()


def main():
    parser = argparse.ArgumentParser(description="Orchestral OpenCode-Style Terminal UI Monitor")
    parser.add_argument("--core-url", default=os.environ.get("ORCHESTRAL_CORE_URL", "http://127.0.0.1:8088"), help="Orchestral Citadel API base URL")
    parser.add_argument("--api-key", default=os.environ.get("ORCHESTRAL_API_KEY", ""), help="Orchestral Citadel API Key (or set ORCHESTRAL_API_KEY)")
    parser.add_argument("--duration", type=int, default=5, help="Seconds to run (0 for infinite loop)")
    parser.add_argument("--snapshot-only", action="store_true", help="Print single snapshot and exit")
    args = parser.parse_args()

    tui = OpenCodeStyleTUI(core_url=args.core_url, api_key=args.api_key)
    if args.snapshot_only:
        tui.console.print(tui.render_snapshot())
    else:
        tui.run_live(duration_seconds=args.duration)


if __name__ == "__main__":
    main()
