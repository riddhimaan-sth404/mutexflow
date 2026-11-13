import socket
import ssl
from typing import Dict, Any, List
from orchestral_worker.tools.base import BaseSecurityTool
from orchestral_worker.guard.hitl import RiskTier


class NativePortScannerTool(BaseSecurityTool):
    """
    Native non-blocking TCP socket scanner and banner grabber.
    Enables Recon agents to discover open services without external binaries.
    """

    def __init__(self, **kwargs):
        super().__init__(
            name="native_port_scanner",
            description="Performs TCP port discovery and service banner extraction",
            risk_tier=RiskTier.ACTIVE,
            **kwargs,
        )

    def _grab_banner(self, host: str, port: int, timeout: float = 1.0) -> str:
        try:
            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
                s.settimeout(timeout)
                s.connect((host, port))
                if port in (443, 8443):
                    # A TLS endpoint needs a real handshake, not a plaintext HEAD.
                    context = ssl._create_unverified_context()
                    with context.wrap_socket(s, server_hostname=host) as tls:
                        cert = tls.getpeercert()
                        subject = ""
                        if cert:
                            subject = ", ".join(
                                f"{k}={v}" for part in cert.get("subject", ()) for k, v in part
                            )
                        return f"TLS {tls.version()} {subject}".strip()
                if port in (80, 8080):
                    s.sendall(b"HEAD / HTTP/1.0\r\nHost: " + host.encode() + b"\r\n\r\n")
                else:
                    s.sendall(b"\r\n")
                banner = s.recv(1024).decode("utf-8", errors="ignore").strip()
                return banner.split("\r\n")[0] if banner else "Unknown Service"
        except Exception:
            return "No Banner"

    def _run(self, target: str, ports: List[int] = None, timeout: float = 0.5, **kwargs) -> Dict[str, Any]:
        ports = ports or [21, 22, 23, 25, 53, 80, 110, 143, 443, 3306, 5432, 6379, 8000, 8080, 8443, 9000]
        open_ports = []

        host = target.replace("http://", "").replace("https://", "").split("/")[0].split(":")[0]

        for p in ports:
            try:
                with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
                    s.settimeout(timeout)
                    result = s.connect_ex((host, p))
                    if result == 0:
                        banner = self._grab_banner(host, p)
                        open_ports.append({"port": p, "state": "open", "banner": banner})
            except Exception:
                continue

        return {
            "tool": self.name,
            "target": host,
            "scanned_ports_count": len(ports),
            "open_ports": open_ports,
            "total_open": len(open_ports),
        }
