use sha2::{Digest, Sha256};

// Deterministic CSRF token derived from session token
pub fn generate_csrf_token(session_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(session_id.as_bytes());
    hasher.update(b"csrf-salt-bzod-2026");
    hex::encode(hasher.finalize())
}

// Verify CSRF token
pub fn verify_csrf(session_id: &str, submitted_token: &str) -> bool {
    let expected = generate_csrf_token(session_id);
    expected == submitted_token
}
