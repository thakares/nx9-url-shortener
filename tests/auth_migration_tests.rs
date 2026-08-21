use axum::extract::{ConnectInfo, Form, State};
use axum::http::HeaderMap;
use axum_extra::extract::cookie::{Cookie, CookieJar};
use chrono::Utc;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bzod::analytics::AnalyticsQueue;
use bzod::auth::session::authenticate_api_key;
use bzod::config::Config;
use bzod::db::admin::{create_api_key, create_session};
use bzod::db::migrations::{run_migrations, ADMIN_MIGRATIONS, SYSTEM_MIGRATIONS, USERS_MIGRATIONS};
use bzod::db::sqlite::get_user_version;
use bzod::db::users::{create_admin_user, create_user, get_user_by_username};
use bzod::db::Db;
use bzod::web::admin::{dashboard_get, login_post, LoginForm};

fn create_temp_config(temp_dir: PathBuf) -> Config {
    let mut config = Config::load();
    config.data_dir = temp_dir.clone();
    config.backup_dir = temp_dir.clone();
    config.admin_username = "admin".to_string();
    config.base_url = Some("http://localhost:8080".to_string());
    config
}

fn compute_sha256(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

fn build_state(config: Config) -> (Db, bzod::state::AppState) {
    let db = Db::init(&config).expect("Failed to init Db");
    let (tx, rx) = tokio::sync::watch::channel(false);
    Box::leak(Box::new(tx));
    let (queue, _) = AnalyticsQueue::new(db.clone(), 1000, rx);
    let state = bzod::state::AppState {
        admin_db: db.admin.clone(),
        system_db: db.system.clone(),
        users_db: db.users.clone(),
        user_dbs: Arc::new(Mutex::new(HashMap::new())),
        db: db.clone(),
        config,
        analytics_queue: queue,
        start_time: Instant::now(),
    };
    (db, state)
}

#[test]
fn test_migration_idempotent() {
    let mut conn = Connection::open_in_memory().unwrap();
    run_migrations(&mut conn, "users", USERS_MIGRATIONS, None).unwrap();
    run_migrations(&mut conn, "users", USERS_MIGRATIONS, None).unwrap();
    assert_eq!(
        get_user_version(&conn).unwrap(),
        USERS_MIGRATIONS.last().unwrap().version
    );
}

#[test]
fn test_users_db_schema_version_2() {
    let mut conn = Connection::open_in_memory().unwrap();
    run_migrations(&mut conn, "users", USERS_MIGRATIONS, None).unwrap();
    assert_eq!(
        get_user_version(&conn).unwrap(),
        USERS_MIGRATIONS.last().unwrap().version
    );
}

#[test]
fn test_admin_db_schema_version_2() {
    let mut conn = Connection::open_in_memory().unwrap();
    run_migrations(&mut conn, "admin", ADMIN_MIGRATIONS, None).unwrap();
    assert_eq!(get_user_version(&conn).unwrap(), 2);
}

#[tokio::test]
async fn test_admin_account_migration() {
    let temp_dir = std::env::temp_dir().join(format!(
        "bzod_test_admin_migration_{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let users_path = temp_dir.join("admin").join("users.db");
    std::fs::create_dir_all(users_path.parent().unwrap()).unwrap();
    let mut conn = Connection::open(&users_path).unwrap();
    run_migrations(&mut conn, "users", USERS_MIGRATIONS, None).unwrap();
    conn.pragma_update(None, "user_version", 1).unwrap();

    let password_hash = bzod::auth::hash_password("securepass").unwrap();
    conn.execute(
        "INSERT INTO users (username, password_hash, status, created_at, account_type) VALUES (?1, ?2, ?3, ?4, ?5);",
        rusqlite::params!["admin", password_hash, "active", Utc::now().to_rfc3339(), "standard"],
    )
    .unwrap();

    let mut system_conn = Connection::open(temp_dir.join("admin/system.db")).unwrap();
    run_migrations(&mut system_conn, "system", SYSTEM_MIGRATIONS, None).unwrap();
    drop(system_conn);

    let config = create_temp_config(temp_dir.clone());
    let db = Db::init(&config).expect("Db::init should repair an existing admin account");

    let conn = db.users.lock().unwrap();
    let user = get_user_by_username(&conn, "admin").unwrap().unwrap();
    println!("admin account_type = {}", user.account_type);
    println!("admin status = {}", user.status);
    let schema_version: i64 = conn
        .query_row("PRAGMA user_version;", [], |row| row.get(0))
        .unwrap();
    println!("users db version = {}", schema_version);
    assert_eq!(user.account_type, "admin");
    assert_eq!(user.status, "active");

    let system_conn = db.system.lock().unwrap();
    let count: i64 = system_conn
        .query_row(
            "SELECT COUNT(*) FROM audit_events WHERE action = 'migration_repair' AND actor = 'admin' AND object_type = 'users' AND object_id = 'admin';",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_admin_login() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_admin_login_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let (_db, state) = build_state(config);

    let password = "adminpassword";
    let hash = bzod::auth::hash_password(password).unwrap();
    {
        let conn = state.users_db.lock().unwrap();
        create_admin_user(&conn, "admin", &hash).unwrap();
    }

    let csrf_token = compute_sha256("csrf-token");
    let jar = CookieJar::new().add(Cookie::new("bzod_temp_csrf", csrf_token.clone()));
    let mut headers = HeaderMap::new();
    headers.insert("user-agent", "test-agent".parse().unwrap());
    let connect_info = Some(ConnectInfo(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        8080,
    )));
    let form = LoginForm {
        username: "admin".to_string(),
        password: password.to_string(),
        csrf_token,
    };

    let response = login_post(
        State(state.clone()),
        jar.clone(),
        headers,
        connect_info,
        Form(form),
    )
    .await;
    assert!(response.status().is_redirection());
    assert_eq!(
        response.headers().get("location").unwrap(),
        "/admin/dashboard"
    );

    let conn = state.users_db.lock().unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions;", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_disabled_user_login_rejected() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_disabled_login_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let (_db, state) = build_state(config);

    let password = "disabledpass";
    let hash = bzod::auth::hash_password(password).unwrap();
    {
        let conn = state.users_db.lock().unwrap();
        conn.execute(
            "INSERT INTO users (username, password_hash, status, created_at, account_type) VALUES (?1, ?2, ?3, ?4, ?5);",
            rusqlite::params!["disabled_user", hash, "disabled", Utc::now().to_rfc3339(), "system"],
        )
        .unwrap();
    }

    let csrf_token = compute_sha256("csrf-token");
    let jar = CookieJar::new().add(Cookie::new("bzod_temp_csrf", csrf_token.clone()));
    let mut headers = HeaderMap::new();
    headers.insert("user-agent", "test-agent".parse().unwrap());
    let connect_info = Some(ConnectInfo(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        8080,
    )));
    let form = LoginForm {
        username: "disabled_user".to_string(),
        password: password.to_string(),
        csrf_token,
    };

    let response = login_post(
        State(state.clone()),
        jar.clone(),
        headers,
        connect_info,
        Form(form),
    )
    .await;
    assert!(response.status().is_redirection());
    assert!(response
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("/admin/login"));

    let conn = state.users_db.lock().unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions;", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_session_created_after_login() {
    let temp_dir = std::env::temp_dir().join(format!(
        "bzod_test_session_created_{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let (_db, state) = build_state(config);

    let password = "adminpass2";
    let hash = bzod::auth::hash_password(password).unwrap();
    {
        let conn = state.users_db.lock().unwrap();
        create_admin_user(&conn, "admin", &hash).unwrap();
    }

    let csrf_token = compute_sha256("csrf-token");
    let jar = CookieJar::new().add(Cookie::new("bzod_temp_csrf", csrf_token.clone()));
    let mut headers = HeaderMap::new();
    headers.insert("user-agent", "test-agent".parse().unwrap());
    let connect_info = Some(ConnectInfo(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        8080,
    )));
    let form = LoginForm {
        username: "admin".to_string(),
        password: password.to_string(),
        csrf_token,
    };

    let response = login_post(
        State(state.clone()),
        jar.clone(),
        headers,
        connect_info,
        Form(form),
    )
    .await;
    assert!(response.status().is_redirection());

    let conn = state.users_db.lock().unwrap();
    let row_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions;", [], |row| row.get(0))
        .unwrap();
    assert_eq!(row_count, 1);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_admin_dashboard_access() {
    let temp_dir = std::env::temp_dir().join(format!(
        "bzod_test_dashboard_access_{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let (_db, state) = build_state(config);

    let password = "adminpass3";
    let hash = bzod::auth::hash_password(password).unwrap();
    let user_id = {
        let conn = state.users_db.lock().unwrap();
        let user = create_admin_user(&conn, "admin", &hash).unwrap();
        user.id
    };

    let session_token = "session_access_token";
    {
        let conn = state.users_db.lock().unwrap();
        create_session(
            &conn,
            session_token,
            &user_id.to_string(),
            &(Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
        )
        .unwrap();
    }

    let jar = CookieJar::new().add(Cookie::new("bzod_session", session_token));
    let response = dashboard_get(State(state.clone()), jar).await;
    assert!(response.status().is_success());

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_standard_user_cannot_access_admin_panel() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_standard_block_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let (_db, state) = build_state(config);

    let password = "userpass";
    let hash = bzod::auth::hash_password(password).unwrap();
    let user_id = {
        let conn = state.users_db.lock().unwrap();
        create_user(&conn, "regular_user", &hash, "standard", None)
            .unwrap()
            .id
    };

    let session_token = "standard_session_token";
    {
        let conn = state.users_db.lock().unwrap();
        create_session(
            &conn,
            session_token,
            &user_id.to_string(),
            &Utc::now().to_rfc3339(),
        )
        .unwrap();
    }

    let jar = CookieJar::new().add(Cookie::new("bzod_session", session_token));
    let response = dashboard_get(State(state.clone()), jar).await;
    assert!(response.status().is_redirection());
    assert!(response
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("/admin/login"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_bootstrap_only_when_no_admin_exists() {
    let temp_dir = std::env::temp_dir().join(format!(
        "bzod_test_bootstrap_block_{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let mut config = create_temp_config(temp_dir.clone());
    config.bootstrap_password_sha256 = compute_sha256("bootstrap-secret");
    let (_db, state) = build_state(config.clone());

    let mut headers = HeaderMap::new();
    headers.insert("user-agent", "test-agent".parse().unwrap());
    let connect_info = Some(ConnectInfo(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        8080,
    )));

    // Create an existing active admin, which should block bootstrap mode because the user count and admin count are already populated.
    let _existing_admin = {
        let conn = state.users_db.lock().unwrap();
        let user = create_admin_user(
            &conn,
            "admin",
            &bzod::auth::hash_password("admin-password").unwrap(),
        )
        .unwrap();
        create_session(
            &conn,
            "existing_admin_session",
            &user.id.to_string(),
            &(Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
        )
        .unwrap();
        user.id
    };

    let csrf_token = compute_sha256("csrf-token");
    let jar = CookieJar::new().add(Cookie::new("bzod_temp_csrf", csrf_token.clone()));
    let form = LoginForm {
        username: "admin".to_string(),
        password: "bootstrap-secret".to_string(),
        csrf_token,
    };
    let response = login_post(
        State(state.clone()),
        jar.clone(),
        headers,
        connect_info,
        Form(form),
    )
    .await;
    assert!(response.status().is_redirection());
    assert!(response
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("/admin/login"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_system_account_cannot_access_admin_panel() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_system_block_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let (_db, state) = build_state(config);

    let password = "systempass";
    let hash = bzod::auth::hash_password(password).unwrap();
    let user_id = {
        let conn = state.users_db.lock().unwrap();
        conn.execute(
            "INSERT INTO users (username, password_hash, status, created_at, account_type) VALUES (?1, ?2, ?3, ?4, ?5);",
            rusqlite::params!["system_user", hash, "active", Utc::now().to_rfc3339(), "system"],
        )
        .unwrap();
        conn.last_insert_rowid()
    };

    let session_token = "system_session_token";
    {
        let conn = state.users_db.lock().unwrap();
        create_session(
            &conn,
            session_token,
            &user_id.to_string(),
            &(Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
        )
        .unwrap();
    }

    let jar = CookieJar::new().add(Cookie::new("bzod_session", session_token));
    let response = dashboard_get(State(state.clone()), jar).await;
    assert!(response.status().is_redirection());
    assert!(response
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("/admin/login"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_reserved_username_rejected() {
    let temp_dir = std::env::temp_dir().join(format!(
        "bzod_test_reserved_username_{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let (_db, state) = build_state(config);

    let password_hash = bzod::auth::hash_password("test").unwrap();
    let create_result = create_user(
        &state.users_db.lock().unwrap(),
        "admin",
        &password_hash,
        "standard",
        None,
    );
    assert!(create_result.is_err());

    let admin_result = create_admin_user(&state.users_db.lock().unwrap(), "admin", &password_hash);
    assert!(admin_result.is_ok());

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_existing_installation_can_login_after_upgrade() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_upgrade_login_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let admin_dir = temp_dir.join("admin");
    std::fs::create_dir_all(&admin_dir).unwrap();
    let users_db_path = admin_dir.join("users.db");
    let mut users_conn = Connection::open(&users_db_path).unwrap();
    run_migrations(&mut users_conn, "users", USERS_MIGRATIONS, None).unwrap();
    users_conn.pragma_update(None, "user_version", 1).unwrap();
    let password = "upgradetest";
    let password_hash = bzod::auth::hash_password(password).unwrap();
    users_conn
        .execute(
            "INSERT INTO users (username, password_hash, status, created_at, account_type) VALUES (?1, ?2, ?3, ?4, ?5);",
            rusqlite::params!["admin", password_hash, "active", Utc::now().to_rfc3339(), "standard"],
        )
        .unwrap();

    let _ = Db::init(&config).expect("Db::init should repair legacy install");
    let (_db, state) = build_state(config.clone());

    {
        let conn = state.users_db.lock().unwrap();
        let user_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM users;", [], |row| row.get(0))
            .unwrap();
        let admin_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM users WHERE account_type = 'admin';",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let active_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM users WHERE status = 'active';",
                [],
                |row| row.get(0),
            )
            .unwrap();
        println!(
            "upgrade login counts users={}, admin={}, active={}",
            user_count, admin_count, active_count
        );
        let mut stmt = conn
            .prepare("SELECT username, status, account_type FROM users;")
            .unwrap();
        let mut rows = stmt.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            let username: String = row.get(0).unwrap();
            let status: String = row.get(1).unwrap();
            let account_type: String = row.get(2).unwrap();
            println!("user row: {} {} {}", username, status, account_type);
        }
    }

    let csrf_token = compute_sha256("csrf-token");
    let jar = CookieJar::new().add(Cookie::new("bzod_temp_csrf", csrf_token.clone()));
    let mut headers = HeaderMap::new();
    headers.insert("user-agent", "test-agent".parse().unwrap());
    let connect_info = Some(ConnectInfo(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        8080,
    )));
    let form = LoginForm {
        username: "admin".to_string(),
        password: password.to_string(),
        csrf_token,
    };

    let response = login_post(
        State(state.clone()),
        jar.clone(),
        headers,
        connect_info,
        Form(form),
    )
    .await;
    assert!(response.status().is_redirection());
    assert_eq!(
        response.headers().get("location").unwrap(),
        "/admin/dashboard"
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_expired_session_cannot_access_dashboard() {
    let temp_dir = std::env::temp_dir().join(format!(
        "bzod_test_expired_session_{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let (_db, state) = build_state(config);

    let password = "adminpass4";
    let hash = bzod::auth::hash_password(password).unwrap();
    let user_id = {
        let conn = state.users_db.lock().unwrap();
        create_admin_user(&conn, "admin", &hash).unwrap().id
    };

    let session_token = "expired_session_token";
    {
        let conn = state.users_db.lock().unwrap();
        create_session(
            &conn,
            session_token,
            &user_id.to_string(),
            &(Utc::now() - chrono::Duration::hours(1)).to_rfc3339(),
        )
        .unwrap();
    }

    let jar = CookieJar::new().add(Cookie::new("bzod_session", session_token));
    let response = dashboard_get(State(state.clone()), jar).await;
    assert!(response.status().is_redirection());
    assert!(response
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("/admin/login"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_api_key_rejected_for_non_admin_user() {
    let temp_dir = std::env::temp_dir().join(format!(
        "bzod_test_api_key_non_admin_{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let (db, _state) = build_state(config);

    let password = "standardpass";
    let hash = bzod::auth::hash_password(password).unwrap();
    let user_id = {
        let conn = db.users.lock().unwrap();
        create_user(&conn, "standard_user", &hash, "standard", None)
            .unwrap()
            .id
    };

    let key_secret = "nonadmin-api-key";
    let mut hasher = Sha256::new();
    hasher.update(key_secret.as_bytes());
    let hashed_key = hex::encode(hasher.finalize());
    {
        let conn = db.admin.lock().unwrap();
        create_api_key(&conn, &user_id.to_string(), "test-key", &hashed_key).unwrap();
    }

    let auth_header = format!("Bearer {}", key_secret);
    let admin_conn = db.admin.lock().unwrap();
    let users_conn = db.users.lock().unwrap();
    let auth_res = authenticate_api_key(&admin_conn, &users_conn, &auth_header).unwrap();
    assert!(auth_res.is_none());

    let _ = std::fs::remove_dir_all(&temp_dir);
}
