//! URL validation to prevent SSRF attacks.
//!
//! Rejects URLs pointing to private/loopback addresses while allowing
//! arbitrary non-IP hostnames (e.g. "dash-node-1:8080").

use std::net::IpAddr;

use crate::error::HubError;

/// Validate that a URL is safe to use as a callback or endpoint.
///
/// Rules:
/// - Scheme must be `http` or `https`.
/// - Host must not be a known private/loopback IP or "localhost".
/// - Non-IP hostnames (e.g. service names) are allowed.
pub fn validate_url(url: &str) -> Result<(), HubError> {
    // Extract scheme
    let rest = if let Some(rest) = url.strip_prefix("https://") {
        rest
    } else if let Some(rest) = url.strip_prefix("http://") {
        rest
    } else {
        return Err(HubError::Validation(
            "URL scheme must be http or https".to_string(),
        ));
    };

    // Extract host portion (before first '/' or end of string)
    let authority = rest.split('/').next().unwrap_or(rest);

    // Handle IPv6 bracket notation: [::1]:port
    let host = if authority.starts_with('[') {
        // IPv6 literal
        let end_bracket = authority
            .find(']')
            .ok_or_else(|| HubError::Validation("malformed IPv6 address in URL".to_string()))?;
        &authority[1..end_bracket]
    } else {
        // IPv4 or hostname — strip port if present
        // For IPv4/hostname, port is after the last ':'
        match authority.rfind(':') {
            Some(pos) => &authority[..pos],
            None => authority,
        }
    };

    if host.is_empty() {
        return Err(HubError::Validation("URL has empty host".to_string()));
    }

    // Block "localhost" literally
    if host.eq_ignore_ascii_case("localhost") {
        return Err(HubError::Validation(
            "URL must not point to localhost".to_string(),
        ));
    }

    // Try to parse as IP address; if it succeeds, check for private ranges
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_ip(&ip) {
            return Err(HubError::Validation(format!(
                "URL must not point to private/loopback address: {host}"
            )));
        }
    }
    // If it does NOT parse as an IP, it's a hostname — allow it.

    Ok(())
}

/// Returns true if the IP address is in a private, loopback, or link-local range.
fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            // 127.0.0.0/8
            if octets[0] == 127 {
                return true;
            }
            // 10.0.0.0/8
            if octets[0] == 10 {
                return true;
            }
            // 172.16.0.0/12
            if octets[0] == 172 && (16..=31).contains(&octets[1]) {
                return true;
            }
            // 192.168.0.0/16
            if octets[0] == 192 && octets[1] == 168 {
                return true;
            }
            // 169.254.0.0/16 (link-local)
            if octets[0] == 169 && octets[1] == 254 {
                return true;
            }
            // 0.0.0.0
            if octets == [0, 0, 0, 0] {
                return true;
            }
            false
        }
        IpAddr::V6(v6) => {
            // ::1 (loopback)
            if *v6 == std::net::Ipv6Addr::LOCALHOST {
                return true;
            }
            let segments = v6.segments();
            // fc00::/7 (unique local address — IPv6 equivalent of RFC 1918)
            if (segments[0] & 0xfe00) == 0xfc00 {
                return true;
            }
            // fe80::/10 (link-local)
            if (segments[0] & 0xffc0) == 0xfe80 {
                return true;
            }
            // ::ffff:0:0/96 (IPv4-mapped — check the mapped IPv4 against private ranges)
            if let Some(mapped) = v6.to_ipv4_mapped() {
                let octets = mapped.octets();
                if octets[0] == 127
                    || octets[0] == 10
                    || (octets[0] == 172 && (16..=31).contains(&octets[1]))
                    || (octets[0] == 192 && octets[1] == 168)
                    || (octets[0] == 169 && octets[1] == 254)
                    || octets == [0, 0, 0, 0]
                {
                    return true;
                }
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_valid_https_url() {
        assert!(validate_url("https://example.com/webhook").is_ok());
    }

    #[test]
    fn allows_valid_http_url() {
        assert!(validate_url("http://example.com:9090/path").is_ok());
    }

    #[test]
    fn allows_non_ip_hostname() {
        // Hostnames like service names should pass
        assert!(validate_url("http://dash-node-1:8080").is_ok());
        assert!(validate_url("http://my-service.internal:443/hook").is_ok());
    }

    #[test]
    fn rejects_non_http_scheme() {
        assert!(validate_url("ftp://example.com/file").is_err());
        assert!(validate_url("file:///etc/passwd").is_err());
    }

    #[test]
    fn rejects_localhost() {
        assert!(validate_url("http://localhost/hook").is_err());
        assert!(validate_url("http://localhost:8080/hook").is_err());
        assert!(validate_url("http://LOCALHOST/hook").is_err());
    }

    #[test]
    fn rejects_loopback_ipv4() {
        assert!(validate_url("http://127.0.0.1/hook").is_err());
        assert!(validate_url("http://127.0.0.1:9090/hook").is_err());
        assert!(validate_url("http://127.255.255.255/hook").is_err());
    }

    #[test]
    fn rejects_private_10_range() {
        assert!(validate_url("http://10.0.0.1/hook").is_err());
        assert!(validate_url("http://10.255.255.255:80/hook").is_err());
    }

    #[test]
    fn rejects_private_172_range() {
        assert!(validate_url("http://172.16.0.1/hook").is_err());
        assert!(validate_url("http://172.31.255.255/hook").is_err());
    }

    #[test]
    fn allows_172_outside_private_range() {
        assert!(validate_url("http://172.15.0.1/hook").is_ok());
        assert!(validate_url("http://172.32.0.1/hook").is_ok());
    }

    #[test]
    fn rejects_private_192_168_range() {
        assert!(validate_url("http://192.168.0.1/hook").is_err());
        assert!(validate_url("http://192.168.255.255:80/hook").is_err());
    }

    #[test]
    fn rejects_link_local() {
        assert!(validate_url("http://169.254.1.1/hook").is_err());
    }

    #[test]
    fn rejects_zero_address() {
        assert!(validate_url("http://0.0.0.0/hook").is_err());
    }

    #[test]
    fn rejects_ipv6_loopback() {
        assert!(validate_url("http://[::1]:8080/hook").is_err());
    }

    #[test]
    fn rejects_ipv6_ula() {
        assert!(validate_url("http://[fc00::1]:8080/hook").is_err());
        assert!(validate_url("http://[fd12:3456:789a::1]/hook").is_err());
    }

    #[test]
    fn rejects_ipv6_link_local() {
        assert!(validate_url("http://[fe80::1]:8080/hook").is_err());
    }

    #[test]
    fn rejects_ipv4_mapped_ipv6() {
        // ::ffff:10.0.0.1 should be blocked (maps to private IPv4)
        assert!(validate_url("http://[::ffff:10.0.0.1]:8080/hook").is_err());
        assert!(validate_url("http://[::ffff:192.168.1.1]/hook").is_err());
        assert!(validate_url("http://[::ffff:127.0.0.1]/hook").is_err());
    }

    #[test]
    fn allows_ipv4_mapped_public() {
        // ::ffff:8.8.8.8 is a public IP mapped to IPv6 — should pass
        assert!(validate_url("http://[::ffff:8.8.8.8]:8080/hook").is_ok());
    }

    #[test]
    fn allows_public_ip() {
        assert!(validate_url("http://8.8.8.8/hook").is_ok());
        assert!(validate_url("https://203.0.113.1:443/hook").is_ok());
    }

    #[test]
    fn rejects_empty_host() {
        assert!(validate_url("http:///path").is_err());
    }
}
