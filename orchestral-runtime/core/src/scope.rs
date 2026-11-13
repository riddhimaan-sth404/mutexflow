use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Normalize a target into a bare host: strip scheme, credentials, path, port,
/// brackets, and trailing dot.
pub fn normalize_host(target: &str) -> String {
    let mut value = target.trim().to_string();

    if let Some(idx) = value.find("://") {
        value = value[idx + 3..].to_string();
    }
    // Strip userinfo.
    if let Some(idx) = value.find('@') {
        value = value[idx + 1..].to_string();
    }
    // IPv6 literal in brackets.
    if value.starts_with('[') {
        if let Some(end) = value.find(']') {
            value = value[1..end].to_string();
        }
    } else if value.matches(':').count() == 1 {
        // host:port (not IPv6)
        if let Some(idx) = value.rfind(':') {
            value = value[..idx].to_string();
        }
    }
    if let Some(idx) = value.find('/') {
        value = value[..idx].to_string();
    }
    if let Some(idx) = value.find('?') {
        value = value[..idx].to_string();
    }
    value.trim_end_matches('.').to_lowercase()
}

fn ipv4_in_cidr(ip: Ipv4Addr, network: Ipv4Addr, prefix: u8) -> bool {
    if prefix > 32 {
        return false;
    }
    let mask: u32 = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    (u32::from(ip) & mask) == (u32::from(network) & mask)
}

fn ipv6_in_cidr(ip: Ipv6Addr, network: Ipv6Addr, prefix: u8) -> bool {
    if prefix > 128 {
        return false;
    }
    let mask: u128 = if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - prefix)
    };
    (u128::from(ip) & mask) == (u128::from(network) & mask)
}

fn ip_in_entry(ip: IpAddr, entry: &str) -> bool {
    let entry = entry.trim();
    if let Some((net_part, prefix_part)) = entry.split_once('/') {
        let Ok(prefix) = prefix_part.parse::<u8>() else {
            return false;
        };
        match (ip, net_part.parse::<IpAddr>()) {
            (IpAddr::V4(ip), Ok(IpAddr::V4(net))) => ipv4_in_cidr(ip, net, prefix),
            (IpAddr::V6(ip), Ok(IpAddr::V6(net))) => ipv6_in_cidr(ip, net, prefix),
            _ => false,
        }
    } else {
        entry.parse::<IpAddr>().map(|net| net == ip).unwrap_or(false)
    }
}

fn domain_matches(host: &str, pattern: &str) -> bool {
    let pattern = pattern.trim().trim_end_matches('.').to_lowercase();
    if pattern == "*" {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix("*.") {
        host == suffix || host.ends_with(&format!(".{}", suffix))
    } else if let Some(suffix) = pattern.strip_prefix('.') {
        host == suffix || host.ends_with(&format!(".{}", suffix))
    } else {
        host == pattern
    }
}

/// Returns true when `target` is permitted by `target_scope`.
///
/// `target_scope` is a comma-separated list of CIDRs, bare IPs, domains, and
/// wildcard domains (`*.example.com`). `*` permits everything (admin use only).
pub fn target_in_scope(target_scope: &str, target: &str) -> bool {
    let host = normalize_host(target);
    if host.is_empty() {
        return false;
    }

    let parsed_ip = host.parse::<IpAddr>().ok();

    for entry in target_scope.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        if entry == "*" {
            return true;
        }
        if let Some(ip) = parsed_ip {
            if ip_in_entry(ip, entry) {
                return true;
            }
        }
        if domain_matches(&host, entry) {
            return true;
        }
    }
    false
}

/// Returns true when `tool` is permitted by an allowed-tools list.
/// An empty list or `*` permits everything.
pub fn tool_allowed(allowed_tools: &[String], tool: &str) -> bool {
    if allowed_tools.is_empty() {
        return true;
    }
    allowed_tools.iter().any(|allowed| {
        let allowed = allowed.trim();
        allowed == "*" || allowed == tool || allowed == format!("kali:{}", tool)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_ip_scope_matches() {
        assert!(target_in_scope("127.0.0.1", "127.0.0.1"));
        assert!(target_in_scope("127.0.0.1/32", "127.0.0.1"));
        assert!(!target_in_scope("127.0.0.1/32", "evil.example.com"));
    }

    #[test]
    fn cidr_scope_matches() {
        assert!(target_in_scope("10.0.0.0/8", "10.1.2.3"));
        assert!(!target_in_scope("10.0.0.0/8", "11.1.2.3"));
        assert!(!target_in_scope("210.0.0.0/8", "10.1.2.3"));
    }

    #[test]
    fn domains_and_urls() {
        assert!(target_in_scope("*.example.com", "https://api.example.com:443/x"));
        assert!(target_in_scope("example.com", "example.com."));
        assert!(!target_in_scope("*.example.com", "example.org"));
    }
}
