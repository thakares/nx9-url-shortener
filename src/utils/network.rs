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
