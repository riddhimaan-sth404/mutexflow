import ipaddress
import socket
from typing import List, Optional, Set
from urllib.parse import urlparse


class ScopeViolationError(Exception):
    """Raised when an action or target violates defined scope constraints."""
    pass


# Networks that should never be reachable during an engagement unless the
# operator explicitly opts into private addressing.
_PRIVATE_NETWORKS = [
    ipaddress.ip_network("10.0.0.0/8"),
    ipaddress.ip_network("172.16.0.0/12"),
    ipaddress.ip_network("192.168.0.0/16"),
    ipaddress.ip_network("127.0.0.0/8"),
    ipaddress.ip_network("169.254.0.0/16"),
    ipaddress.ip_network("100.64.0.0/10"),
    ipaddress.ip_network("::1/128"),
    ipaddress.ip_network("fc00::/7"),
    ipaddress.ip_network("fe80::/10"),
]


def _normalize_host(host: str) -> str:
    """Lowercase, strip trailing dots, and IDNA-encode a hostname."""
    host = (host or "").strip().rstrip(".")
    if not host:
        return host
    try:
        return host.encode("idna").decode("ascii").lower()
    except UnicodeError:
        return host.lower()


def _parse_network(value: str) -> Optional[ipaddress._BaseNetwork]:
    """Parse an IP or CIDR string, returning None for hostnames."""
    value = (value or "").strip()
    if not value:
        return None
    try:
        if "/" in value:
            return ipaddress.ip_network(value, strict=False)
        return ipaddress.ip_network(f"{value}/32" if "." in value else f"{value}/128", strict=False)
    except ValueError:
        return None


def _is_private_network(net: ipaddress._BaseNetwork) -> bool:
    """Return True when a network is wholly inside a private/loopback range."""
    if net.is_private or net.is_loopback or net.is_link_local:
        return True
    return any(net.subnet_of(private) for private in _PRIVATE_NETWORKS
               if net.version == private.version)


class ScopeGuard:
    """
    Deterministic Scope Enforcer for Security Research Agents.
    Guarantees that agents strictly target authorized infrastructure,
    preventing unauthorized scans or collateral damage.
    """

    def __init__(
        self,
        allowed_domains: Optional[List[str]] = None,
        allowed_cidrs: Optional[List[str]] = None,
        denied_cidrs: Optional[List[str]] = None,
        allow_private_ips: bool = False,
        allowlist: Optional[List[str]] = None,
        resolve_dns: bool = True,
    ):
        raw_domains = list(allowed_domains or [])
        raw_cidrs = list(allowed_cidrs or [])

        # Convenience parser if a single allowlist is passed. Use the ipaddress
        # module instead of string heuristics so bare IPs (127.0.0.1) and IPv6
        # literals are classified correctly.
        if allowlist:
            for item in allowlist:
                item_str = str(item).strip()
                if not item_str:
                    continue
                net = _parse_network(item_str)
                if net is not None:
                    raw_cidrs.append(str(net))
                else:
                    raw_domains.append(item_str)

        self.resolve_dns = resolve_dns

        # Auto-detect if private/loopback IPs are explicitly permitted. This
        # must compare parsed networks, not substrings: "210.0.0.0/8" contains
        # "10." but is public.
        for c in raw_cidrs:
            net = _parse_network(c)
            if net is not None and _is_private_network(net):
                allow_private_ips = True

        self.allowed_domains = [_normalize_host(d) for d in raw_domains]
        self.allowed_cidrs = [
            ipaddress.ip_network(c.strip(), strict=False) for c in raw_cidrs
        ]

        # Default denied: cloud metadata endpoints, link-local, multicast,
        # broadcast, and (unless explicitly allowed) private/loopback ranges.
        default_denied = [
            "169.254.169.254/32",
            "169.254.0.0/16",
            "224.0.0.0/4",
            "255.255.255.255/32",
            "0.0.0.0/8",
            "::/128",
            "ff00::/8",
        ]
        if not allow_private_ips:
            default_denied.extend(str(n) for n in _PRIVATE_NETWORKS)

        all_denied = default_denied + (denied_cidrs or [])
        self.denied_cidrs = [
            ipaddress.ip_network(c.strip(), strict=False) for c in all_denied
        ]

    @property
    def allowlist(self) -> List[str]:
        return list(self.allowed_domains) + [str(c) for c in self.allowed_cidrs]

    def _extract_host(self, target: str) -> str:
        target = target.strip()
        if "://" in target:
            parsed = urlparse(target)
            return parsed.hostname or target
        if target.startswith("[") and "]" in target:  # IPv6 literal
            return target[1:target.index("]")]
        if ":" in target and target.count(":") == 1:  # host:port (not IPv6)
            return target.split(":")[0]
        return target

    def _check_ip(self, ip: ipaddress._BaseAddress) -> None:
        for denied_net in self.denied_cidrs:
            if ip.version == denied_net.version and ip in denied_net:
                raise ScopeViolationError(
                    f"Target IP {ip} falls inside explicitly denied network: {denied_net}"
                )

    def _resolve_host(self, host: str) -> Set[str]:
        """Resolve a hostname to its A/AAAA records, if DNS is enabled."""
        if not self.resolve_dns:
            return set()
        try:
            infos = socket.getaddrinfo(host, None)
        except (socket.gaierror, UnicodeError, OSError):
            return set()
        return {info[4][0] for info in infos}

    def _validate_resolved(self, host: str) -> None:
        """Reject allowlisted domains that resolve into denied networks.

        This catches cloud-metadata aliases and DNS-rebinding style bypasses
        where the literal hostname looks benign but resolves internally.
        """
        for addr in self._resolve_host(host):
            try:
                ip = ipaddress.ip_address(addr)
            except ValueError:
                continue
            self._check_ip(ip)

    def validate_target(self, target: str) -> bool:
        """
        Validates whether target is permitted under current engagement scope.
        Raises ScopeViolationError if target is out of scope or explicitly denied.
        """
        # Local paths for SAST / static inspection
        if target in (".", "./", "/") or target.startswith(("./", "../", "/")):
            return True

        host = self._extract_host(target)
        if not host:
            raise ScopeViolationError("Target is empty after parsing.")

        # Check if target is an IP address
        try:
            ip = ipaddress.ip_address(host)
        except ValueError:
            ip = None

        if ip is not None:
            # 1. Deny check
            self._check_ip(ip)

            # 2. Allow check
            if not self.allowed_cidrs:
                raise ScopeViolationError(
                    f"Target IP {host} rejected: No CIDR ranges configured in scope allowlist."
                )

            is_allowed = any(
                ip.version == net.version and ip in net for net in self.allowed_cidrs
            )
            if not is_allowed:
                allowed_str = ", ".join(str(n) for n in self.allowed_cidrs)
                raise ScopeViolationError(
                    f"Target IP {host} is NOT in authorized CIDR list: [{allowed_str}]"
                )
            return True

        # Target is a domain / hostname
        host_lower = _normalize_host(host)
        if not self.allowed_domains:
            raise ScopeViolationError(
                f"Target domain '{host}' rejected: No domains configured in scope allowlist."
            )

        for pattern in self.allowed_domains:
            if pattern.startswith("*."):
                suffix = pattern[1:]  # e.g., '.internal.local'
                if host_lower.endswith(suffix) or host_lower == pattern[2:]:
                    self._validate_resolved(host)
                    return True
            elif host_lower == pattern:
                self._validate_resolved(host)
                return True

        allowed_str = ", ".join(self.allowed_domains)
        raise ScopeViolationError(
            f"Target domain '{host}' is NOT in authorized domains list: [{allowed_str}]"
        )
