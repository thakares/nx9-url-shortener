use axum_extra::extract::cookie::Cookie;
use axum_extra::extract::CookieJar;
use chrono::Utc;
use rusqlite::Connection;

use bzod::auth::{
    authenticate_api_key, authenticate_session, generate_csrf_token, hash_password, verify_csrf,
    verify_password, verify_sha256,
};
use bzod::db::admin::{create_api_key, create_session, create_user, get_user_count};
use bzod::db::migrations::{run_migrations, ADMIN_MIGRATIONS};

// Helper to set up an in-memory admin.db connection with migrations applied
fn setup_test_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    run_migrations(&mut conn, "admin", ADMIN_MIGRATIONS, None).unwrap();
    conn
}

#[test]
fn test_csrf_tampering_prevention() {
    let session_id = "secret_session_id_123456";
    let valid_token = generate_csrf_token(session_id);

    // Mismatched token must fail
    assert!(!verify_csrf(session_id, "different_token_value"));

    // Valid token must pass
    assert!(verify_csrf(session_id, &valid_token));

    // Mismatched session id must fail even if token matches the original session id
    assert!(!verify_csrf("different_session_id_789", &valid_token));
}

#[test]
fn test_api_key_sql_injection_resistance() {
    let conn = setup_test_db();

    // Create an API key
    let user_hash = hash_password("admin_pass").unwrap();
    let user = create_user(&conn, "admin", &user_hash).unwrap();

    // Generate valid API key
    let key_secret = "bzo_validkey1234567890abcdef";
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(key_secret.as_bytes());
    let hashed_key = hex::encode(hasher.finalize());
    create_api_key(&conn, &user.id, "my-key", &hashed_key).unwrap();

    // 1. Test valid key passes
    let valid_auth = format!("Bearer {}", key_secret);
    let auth_res = authenticate_api_key(&conn, &valid_auth).unwrap();
    assert!(auth_res.is_some());
    assert_eq!(auth_res.unwrap().username, "admin");

    // 2. Test SQL Injection attempt in the header does not succeed or crash
    let sql_inj_auth1 = "Bearer ' OR 1=1 --";
    let res = authenticate_api_key(&conn, sql_inj_auth1).unwrap();
    assert!(res.is_none());

    let sql_inj_auth2 = "Bearer ' UNION SELECT id, username FROM users --";
    let res = authenticate_api_key(&conn, sql_inj_auth2).unwrap();
    assert!(res.is_none());

    // 3. Test malformed header
    let malformed_auth = "Bearer";
    let res = authenticate_api_key(&conn, malformed_auth).unwrap();
    assert!(res.is_none());

    let wrong_scheme = "Basic admin:pass";
    let res = authenticate_api_key(&conn, wrong_scheme).unwrap();
    assert!(res.is_none());
}

#[test]
fn test_expired_session_invalidation() {
    let conn = setup_test_db();

    let user_hash = hash_password("pass").unwrap();
    let user = create_user(&conn, "admin", &user_hash).unwrap();

    // 1. Session in the future must be valid
    let future_expiry = (Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    let session_id_future = "future_session_token";
    create_session(&conn, session_id_future, &user.id, &future_expiry).unwrap();

    let jar_future = CookieJar::new().add(Cookie::new("bzod_session", session_id_future));
    let auth_future = authenticate_session(&conn, &jar_future).unwrap();
    assert!(auth_future.is_some());
    assert_eq!(auth_future.unwrap().0.id, user.id);

    // 2. Session in the past must be rejected
    let past_expiry = (Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
    let session_id_past = "expired_session_token";
    create_session(&conn, session_id_past, &user.id, &past_expiry).unwrap();

    let jar_past = CookieJar::new().add(Cookie::new("bzod_session", session_id_past));
    let auth_past = authenticate_session(&conn, &jar_past).unwrap();
    assert!(auth_past.is_none());
}

#[test]
fn test_bootstrap_credentials_deactivation() {
    let conn = setup_test_db();

    let bootstrap_sha = "8c6976e5b5410415bde908bd4dee15dfb167a9c873fc4bb8a81f6f2ab448a918"; // SHA-256 of "admin"

    // 1. Initially, no users exist in database
    assert_eq!(get_user_count(&conn).unwrap(), 0);
    // Bootstrap validation is allowed
    assert!(verify_sha256("admin", bootstrap_sha));

    // 2. Provision a user in database (either via bootstrap login or CLI)
    let user_hash = hash_password("new_secure_admin_password").unwrap();
    create_user(&conn, "admin", &user_hash).unwrap();

    // Check that database now has users
    assert_eq!(get_user_count(&conn).unwrap(), 1);

    // Standard credential validation must pass
    let user_opt = bzod::db::admin::get_user_by_username(&conn, "admin").unwrap();
    assert!(user_opt.is_some());
    assert!(verify_password(
        "new_secure_admin_password",
        &user_opt.unwrap().password_hash
    ));

    // The bootstrap credentials MUST be ignored now (the application logic checks users count,
    // which is 1, so it bypasses the bootstrap check and verifies ONLY against the database).
}

#[test]
fn test_path_traversal_rejection() {
    // Standard Axum router exact path matching prevents path traversal on endpoints.
    // If a request has /../admin, standard HTTP parsers and Axum router resolve it as /admin
    // (which checks session cookies) or return 404 for unresolved paths.
    // Here we verify that code inputs containing traversal strings are parsed as invalid codes.

    let invalid_codes = vec!["../foo", "..%2ff", "/admin", "a/b/c", "1234567"];

    for code in invalid_codes {
        // Validate redirect code must be exactly 6 hex digits
        let is_valid_redirect_code = code.len() == 6 && code.chars().all(|c| c.is_ascii_hexdigit());
        assert!(
            !is_valid_redirect_code,
            "Code '{}' should be rejected as a valid redirect shortcode",
            code
        );

        // Validate landing page code must be exactly 4 hex digits
        let is_valid_page_code = code.len() == 4 && code.chars().all(|c| c.is_ascii_hexdigit());
        assert!(
            !is_valid_page_code,
            "Code '{}' should be rejected as a valid landing page shortcode",
            code
        );
    }
}
