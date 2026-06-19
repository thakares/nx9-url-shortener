use axum_extra::extract::cookie::Cookie;
use axum_extra::extract::CookieJar;
use bzod::config::Config;
use bzod::db::Db;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

fn create_temp_config(temp_dir: PathBuf) -> Config {
    let mut config = Config::load();
    config.data_dir = temp_dir.clone();
    config.backup_dir = temp_dir.clone();
    config.base_url = Some("http://bzo.in".to_string());
    config
}

#[tokio::test]
async fn test_session_creation_and_expiry() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_auth_sess_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");

    let _ = bzod::cli::create_user::run(
        Some("testuser".to_string()),
        Some("password123".to_string()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    let user_id = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::get_user_by_username(&conn, "testuser")
            .unwrap()
            .unwrap()
            .id
    };

    // 1. Session created in future must authenticate successfully
    {
        let conn = db.users.lock().unwrap();
        let expires_at = (chrono::Utc::now() + chrono::Duration::seconds(3600)).to_rfc3339();
        bzod::db::users::create_user_session(&conn, "valid_session_123", user_id, &expires_at)
            .unwrap();

        let jar = CookieJar::new().add(Cookie::new("bzod_user_session", "valid_session_123"));
        let auth_res = bzod::auth::session::authenticate_user_session(&conn, &jar).unwrap();
        assert!(auth_res.is_some());
        assert_eq!(auth_res.unwrap().0.id, user_id);
    }

    // 2. Session created in past (expired) must be rejected
    {
        let conn = db.users.lock().unwrap();
        let expires_at = (chrono::Utc::now() - chrono::Duration::seconds(3600)).to_rfc3339();
        bzod::db::users::create_user_session(&conn, "expired_session_123", user_id, &expires_at)
            .unwrap();

        let jar = CookieJar::new().add(Cookie::new("bzod_user_session", "expired_session_123"));
        let auth_res = bzod::auth::session::authenticate_user_session(&conn, &jar).unwrap();
        assert!(auth_res.is_none());
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_api_token_creation_and_revocation() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_auth_token_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");

    let _ = bzod::cli::create_user::run(
        Some("testuser".to_string()),
        Some("password123".to_string()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    let user_id = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::get_user_by_username(&conn, "testuser")
            .unwrap()
            .unwrap()
            .id
    };

    let raw_token = "bzo_testtoken1234567890abcdef";
    let mut hasher = Sha256::new();
    hasher.update(raw_token.as_bytes());
    let token_hash = hex::encode(hasher.finalize());

    // Create API Token
    let token_rec = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::create_user_api_token(&conn, user_id, &token_hash).unwrap()
    };

    // Authenticate with valid token
    {
        let conn = db.users.lock().unwrap();
        let auth_header = format!("Bearer {}", raw_token);
        let actor = bzod::auth::session::authenticate_api_key(
            &db.admin.lock().unwrap(),
            &conn,
            &auth_header,
        )
        .unwrap();
        assert!(actor.is_some());
    }

    // Revoke API Token
    {
        let conn = db.users.lock().unwrap();
        bzod::db::users::delete_user_api_token(&conn, token_rec.id, user_id).unwrap();

        // Verify revoked token is rejected
        let auth_header = format!("Bearer {}", raw_token);
        let actor = bzod::auth::session::authenticate_api_key(
            &db.admin.lock().unwrap(),
            &conn,
            &auth_header,
        )
        .unwrap();
        assert!(actor.is_none());
    }

    let _ = fs::remove_dir_all(&temp_dir);
}
