use crate::db::admin::{
    get_api_key_by_hash, get_session, get_user_by_id, update_api_key_last_used,
};
use crate::models::User;
use axum_extra::extract::CookieJar;
use chrono::Utc;
use rand::{thread_rng, RngCore};
use rusqlite::Connection;
use sha2::{Digest, Sha256};

// Generate a secure random token (hex-encoded)
pub fn generate_token(bytes_len: usize) -> String {
    let mut key = vec![0u8; bytes_len];
    thread_rng().fill_bytes(&mut key);
    hex::encode(key)
}

// Authenticate session from cookies
pub fn authenticate_session(
    conn: &Connection,
    jar: &CookieJar,
) -> Result<Option<(User, String)>, rusqlite::Error> {
    let cookie = match jar.get("bzod_session") {
        Some(c) => c,
        None => return Ok(None),
    };

    let session_id = cookie.value();
    let session = match get_session(conn, session_id)? {
        Some(s) => s,
        None => return Ok(None),
    };

    // Check expiration
    if let Ok(expires) = chrono::DateTime::parse_from_rfc3339(&session.expires_at) {
        if expires.with_timezone(&Utc) < Utc::now() {
            // Expired
            return Ok(None);
        }
    } else {
        return Ok(None);
    }

    // Get user
    if let Some(user) = get_user_by_id(conn, &session.user_id)? {
        Ok(Some((user, session.id)))
    } else {
        Ok(None)
    }
}

// Authenticate API key from Authorization header
pub fn authenticate_api_key(
    conn: &Connection,
    auth_header: &str,
) -> Result<Option<User>, rusqlite::Error> {
    if !auth_header.starts_with("Bearer ") {
        return Ok(None);
    }

    let key = auth_header.trim_start_matches("Bearer ").trim();
    if key.is_empty() {
        return Ok(None);
    }

    // Hash the API key using SHA-256 to compare with stored hash
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    let hashed_key = hex::encode(hasher.finalize());

    if let Some(api_key_rec) = get_api_key_by_hash(conn, &hashed_key)? {
        // Update last used timestamp
        update_api_key_last_used(conn, &api_key_rec.id)?;

        // Get user
        if let Some(user) = get_user_by_id(conn, &api_key_rec.user_id)? {
            return Ok(Some(user));
        }
    }

    Ok(None)
}
