/// Local-network URL validation.
///
/// Used by both the reporting delivery gateway and the connector executor to
/// enforce that HTTP requests are only sent to local/private-network hosts.
///
/// ## Allowed hosts
///
/// - `localhost`, `127.0.0.1`, `::1`
/// - RFC 1918 private ranges: `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`
/// - Additional hostnames from the `LOCAL_NETWORK_ALLOWLIST` env var
///   (comma-separated, e.g. `"mailrelay.internal,im.corp.lan"`)
///
/// ## Rejected
///
/// - Any hostname that resolves to a non-private IP or is not in the allowlist
/// - `https://` URLs (TLS not supported for local gateways)
/// - Malformed URLs
use std::env;
use std::net::IpAddr;

/// Validate that a URL targets a local/private-network host.
///
/// Returns `Ok(())` if the host is allowed, or `Err(description)` explaining
/// why the URL was rejected.
pub fn validate_local_url(url: &str) -> Result<(), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("only http:// URLs are supported, got: {url}"))?;

    let (host_port, _) = rest.split_once('/').unwrap_or((rest, ""));

    // Strip port to get bare host
    let host = if host_port.starts_with('[') {
        // IPv6 bracket notation: [::1]:port
        host_port
            .split_once(']')
            .map(|(h, _)| &h[1..])
            .unwrap_or(host_port)
    } else if let Some(colon_pos) = host_port.rfind(':') {
        &host_port[..colon_pos]
    } else {
        host_port
    };

    if host.is_empty() {
        return Err("URL has an empty host".into());
    }

    // Check well-known local hostnames
    if host == "localhost" {
        return Ok(());
    }

    // Check if it's an IP address in a private range
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_ip(&ip) {
            return Ok(());
        }
        return Err(format!(
            "host '{host}' is not a private/local address; \
             only localhost and RFC 1918 addresses are allowed"
        ));
    }

    // Not an IP literal — check the configurable allowlist
    let allowlist = env::var("LOCAL_NETWORK_ALLOWLIST").unwrap_or_default();
    let allowed: Vec<&str> = allowlist
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    if allowed.iter().any(|&a| a.eq_ignore_ascii_case(host)) {
        return Ok(());
    }

    Err(format!(
        "host '{host}' is not a recognized local/private-network host; \
         add it to LOCAL_NETWORK_ALLOWLIST if it is an internal service"
    ))
}

/// Returns `true` if the IP address is in a private/loopback range.
fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()                          // 127.0.0.0/8
                || v4.octets()[0] == 10                // 10.0.0.0/8
                || (v4.octets()[0] == 172              // 172.16.0.0/12
                    && (v4.octets()[1] & 0xF0) == 16)
                || (v4.octets()[0] == 192              // 192.168.0.0/16
                    && v4.octets()[1] == 168)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback() // ::1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localhost_is_allowed() {
        assert!(validate_local_url("http://localhost:8025/send").is_ok());
        assert!(validate_local_url("http://localhost/path").is_ok());
    }

    #[test]
    fn loopback_ipv4_is_allowed() {
        assert!(validate_local_url("http://127.0.0.1:9090/hooks").is_ok());
        assert!(validate_local_url("http://127.0.0.1/").is_ok());
    }

    #[test]
    fn loopback_ipv6_is_allowed() {
        assert!(validate_local_url("http://[::1]:8080/send").is_ok());
    }

    #[test]
    fn rfc1918_10_is_allowed() {
        assert!(validate_local_url("http://10.0.0.5:8080/sync").is_ok());
        assert!(validate_local_url("http://10.255.255.255/path").is_ok());
    }

    #[test]
    fn rfc1918_172_16_is_allowed() {
        assert!(validate_local_url("http://172.16.0.1:80/api").is_ok());
        assert!(validate_local_url("http://172.31.255.255/api").is_ok());
    }

    #[test]
    fn rfc1918_192_168_is_allowed() {
        assert!(validate_local_url("http://192.168.1.100:3000/webhook").is_ok());
    }

    #[test]
    fn public_ip_is_rejected() {
        let err = validate_local_url("http://8.8.8.8:80/api").unwrap_err();
        assert!(err.contains("not a private"), "{err}");
    }

    #[test]
    fn public_hostname_is_rejected() {
        let err = validate_local_url("http://evil.example.com:80/steal").unwrap_err();
        assert!(err.contains("not a recognized local"), "{err}");
    }

    #[test]
    fn https_is_rejected() {
        let err = validate_local_url("https://localhost:443/path").unwrap_err();
        assert!(err.contains("only http://"), "{err}");
    }

    #[test]
    fn empty_host_is_rejected() {
        let err = validate_local_url("http://:8080/path").unwrap_err();
        assert!(err.contains("empty host"), "{err}");
    }

    #[test]
    fn malformed_url_is_rejected() {
        assert!(validate_local_url("ftp://localhost/file").is_err());
        assert!(validate_local_url("not-a-url").is_err());
    }

    #[test]
    fn non_private_172_is_rejected() {
        // 172.32.x.x is outside the /12 range
        let err = validate_local_url("http://172.32.0.1:80/api").unwrap_err();
        assert!(err.contains("not a private"), "{err}");
    }

    #[test]
    fn allowlist_permits_custom_hostname() {
        env::set_var("LOCAL_NETWORK_ALLOWLIST", "mailrelay.internal,im.corp.lan");
        assert!(validate_local_url("http://mailrelay.internal:8025/send").is_ok());
        assert!(validate_local_url("http://im.corp.lan:9090/hooks").is_ok());
        // Unknown hostname still rejected
        assert!(validate_local_url("http://unknown.host:80/api").is_err());
        env::remove_var("LOCAL_NETWORK_ALLOWLIST");
    }
}
