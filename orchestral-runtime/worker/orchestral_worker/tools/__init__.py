"""Tool packages for the Orchestral worker.

Only the passive secret scanner is imported eagerly. The dormant offensive
and dual-use tool modules are exposed lazily so that importing this package
never loads or registers them. See SECURITY.md at the repository root.
"""
from orchestral_worker.tools.base import BaseSecurityTool
from orchestral_worker.tools.registry import ToolRegistry
from orchestral_worker.tools.secret_scanner import NativeSecretScannerTool

__all__ = [
    "BaseSecurityTool",
    "ToolRegistry",
    "NativeSecretScannerTool",
    "NativePortScannerTool",
    "NativeHttpProbeTool",
    "KaliTerminalTool",
    "CodeQLTool",
]

_LAZY_EXPORTS = {
    "NativePortScannerTool": ("orchestral_worker.tools.port_scanner", "NativePortScannerTool"),
    "NativeHttpProbeTool": ("orchestral_worker.tools.http_probe", "NativeHttpProbeTool"),
    "KaliTerminalTool": ("orchestral_worker.tools.kali_tool", "KaliTerminalTool"),
    "CodeQLTool": ("orchestral_worker.tools.codeql", "CodeQLTool"),
}


def __getattr__(name: str):
    """Lazily import dormant upstream tooling only on explicit attribute use."""
    if name in _LAZY_EXPORTS:
        import importlib

        module_name, attr = _LAZY_EXPORTS[name]
        return getattr(importlib.import_module(module_name), attr)
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")
