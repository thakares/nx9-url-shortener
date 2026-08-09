use axum::{extract::ConnectInfo, http::HeaderMap};
use std::net::SocketAddr;

// Extract client IP address from proxy headers or connection info
pub fn get_client_ip(headers: &HeaderMap, connect_info: Option<ConnectInfo<SocketAddr>>) -> String {
    if let Some(ip) = headers
        .get("cf-connecting-ip")
        .and_then(|h| h.to_str().ok())
    {
        return ip.to_string();
    }
    if let Some(ip) = headers.get("x-real-ip").and_then(|h| h.to_str().ok()) {
        return ip.to_string();
    }
    if let Some(ips) = headers.get("x-forwarded-for").and_then(|h| h.to_str().ok()) {
        if let Some(ip) = ips.split(',').next() {
            return ip.trim().to_string();
        }
    }
    if let Some(ConnectInfo(addr)) = connect_info {
        return addr.ip().to_string();
    }
    "127.0.0.1".to_string()
}

// Helper to safely extract hostname from a Host header, handling IPv6 and ports.
pub(crate) fn extract_hostname(host_header: &str) -> &str {
    if host_header.starts_with('[') {
        if let Some(end_idx) = host_header.find(']') {
            return &host_header[1..end_idx];
        }
    }
    host_header.split(':').next().unwrap_or(host_header)
}

/// Determines whether to set the `Secure` flag on a cookie based on deployment context.
///
/// Policy:
/// 1. If X-Forwarded-Proto is explicitly "https", always enforce Secure=true.
///    (We assume X-Forwarded-Proto is from a trusted proxy. Forged "https" only makes
///    the cookie safer. We never weaken based on X-Forwarded-Proto=http).
/// 2. If the exact Host is a local loopback (localhost, 127.0.0.1, ::1) and we are not
///    explicitly proxied via HTTPS, disable Secure. This prevents browsers from dropping
///    the cookie during local development over cleartext HTTP.
/// 3. Otherwise, fall back to the global `cookie_secure` config (which defaults to true
///    to keep production secure-by-default even if the proxy strips X-Forwarded-Proto).
pub fn resolve_cookie_secure(config_secure: bool, headers: &HeaderMap) -> bool {
    // 1. Explicit HTTPS via reverse proxy
    if let Some(proto) = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
    {
        if proto.eq_ignore_ascii_case("https") {
            return true;
        }
    }

    // 2. Exact loopback development (prevent cookie drop)
    if let Some(host_hdr) = headers.get("host").and_then(|h| h.to_str().ok()) {
        let hostname = extract_hostname(host_hdr);
        if matches!(hostname, "localhost" | "127.0.0.1" | "::1") {
            return false;
        }
    }

    // 3. Global config (normally true)
    config_secure
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn test_extract_hostname() {
        assert_eq!(extract_hostname("localhost"), "localhost");
        assert_eq!(extract_hostname("localhost:8080"), "localhost");
        assert_eq!(extract_hostname("127.0.0.1"), "127.0.0.1");
        assert_eq!(extract_hostname("127.0.0.1:8080"), "127.0.0.1");
        assert_eq!(extract_hostname("[::1]"), "::1");
        assert_eq!(extract_hostname("[::1]:8080"), "::1");
        assert_eq!(extract_hostname("example.com"), "example.com");
        assert_eq!(extract_hostname("example.com:443"), "example.com");
        assert_eq!(
            extract_hostname("localhost.example.com"),
            "localhost.example.com"
        );
    }

    #[test]
    fn test_resolve_cookie_secure() {
        let mut headers = HeaderMap::new();

        // No headers, global config is true -> Secure=true
        assert!(resolve_cookie_secure(true, &headers));
        // No headers, global config is false -> Secure=false
        assert!(!resolve_cookie_secure(false, &headers));

        // Exact loopback matches -> Secure=false
        let loopback_hosts = [
            "localhost",
            "localhost:8080",
            "127.0.0.1",
            "127.0.0.1:8080",
            "[::1]",
            "[::1]:8080",
        ];
        for host in loopback_hosts {
            headers.insert("host", HeaderValue::from_static(host));
            assert!(
                !resolve_cookie_secure(true, &headers),
                "Failed for host: {}",
                host
            );
        }

        // Non-loopback localhost subdomains -> Secure=true (relying on config)
        let public_hosts = ["localhost.example.com", "127.0.0.2", "example.com"];
        for host in public_hosts {
            headers.insert("host", HeaderValue::from_static(host));
            assert!(
                resolve_cookie_secure(true, &headers),
                "Failed for host: {}",
                host
            );
        }

        // X-Forwarded-Proto = https overrides loopback
        headers.insert("host", HeaderValue::from_static("localhost"));
        headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        assert!(resolve_cookie_secure(true, &headers));
        assert!(resolve_cookie_secure(false, &headers)); // Overrides false config too

        // X-Forwarded-Proto = http DOES NOT override global config
        headers.insert("host", HeaderValue::from_static("example.com"));
        headers.insert("x-forwarded-proto", HeaderValue::from_static("http"));
        assert!(resolve_cookie_secure(true, &headers)); // Still true because of config
    }
}
