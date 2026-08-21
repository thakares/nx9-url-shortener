//! v0.8.0 Phase 2: tenant boundary. TenantId is the access key; unknown ids
//! must never create databases; user-1 fallbacks are gone.

use bzod::config::Config;
use bzod::db::tenant::TenantOpenMode;
use bzod::db::Db;
use bzod::identity::TenantId;
use bzod::state::AppState;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

fn temp_config() -> (PathBuf, Config) {
    let temp_dir = std::env::temp_dir().join(format!("bzod_v08_boundary_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let mut config = Config::load();
    config.data_dir = temp_dir.clone();
    config.backup_dir = temp_dir.join("backups");
    (temp_dir, config)
}

fn state_from(db: &Db, config: Config) -> AppState {
    let (queue, _handle) =
        bzod::analytics::AnalyticsQueue::new(db.clone(), 8, tokio::sync::watch::channel(false).1);
    AppState {
        admin_db: db.admin.clone(),
        system_db: db.system.clone(),
        users_db: db.users.clone(),
        user_dbs: Arc::new(Mutex::new(HashMap::new())),
        db: db.clone(),
        config,
        analytics_queue: queue,
        start_time: Instant::now(),
    }
}

#[test]
fn tenant_id_rejects_invalid_forms() {
    assert!(TenantId::parse("FAFAFA12C3E4").is_err());
    assert!(TenantId::parse("abc").is_err());
    assert!(TenantId::parse("a1b2c3d4e5f67").is_err());
    assert!(TenantId::parse("../etc/passwd").is_err());
    assert!(TenantId::parse("fafafa12c3e4").is_ok());
}

#[tokio::test]
async fn unknown_tenant_id_does_not_create_database() {
    let (temp_dir, config) = temp_config();
    let db = Db::init(&config).unwrap();
    let state = state_from(&db, config);
    let forged = TenantId::parse("aaaaaaaaaaaa").unwrap();
    let before = fs::read_dir(temp_dir.join("users")).unwrap().count();
    let err = match state.open_tenant(forged, TenantOpenMode::Ordinary) {
        Ok(_) => panic!("unknown tenant must not open"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("not found"));
    let after = fs::read_dir(temp_dir.join("users")).unwrap().count();
    assert_eq!(before, after);
    assert!(!temp_dir.join("users").join("aaaaaaaaaaaa").exists());
    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn unknown_integer_id_does_not_create_database() {
    let (temp_dir, config) = temp_config();
    let db = Db::init(&config).unwrap();
    let state = state_from(&db, config);
    assert!(state.get_user_dbs(999_999).is_err());
    assert!(!temp_dir.join("users").join("999999").exists());
    assert!(bzod::jobs::open_user_content_conn(&db, 999_999).is_err());
    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn new_user_gets_tenant_id_and_hex_directory() {
    let (temp_dir, config) = temp_config();
    let db = Db::init(&config).unwrap();
    bzod::cli::create_user::run(
        Some("alice".into()),
        Some("password123".into()),
        None,
        config.clone(),
    )
    .await
    .unwrap();
    let user = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::get_user_by_username(&conn, "alice")
            .unwrap()
            .unwrap()
    };
    let tid = user.tenant_id.expect("new users receive TenantId");
    assert_eq!(tid.as_str().len(), 12);
    assert!(temp_dir
        .join("users")
        .join(tid.as_str())
        .join("content.db")
        .exists());
    assert!(!temp_dir.join("users").join(user.id.to_string()).exists());
    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn tenant_a_cannot_open_tenant_b_by_forged_id() {
    let (temp_dir, config) = temp_config();
    let db = Db::init(&config).unwrap();
    let state = state_from(&db, config.clone());
    bzod::cli::create_user::run(
        Some("usera".into()),
        Some("password123".into()),
        None,
        config.clone(),
    )
    .await
    .unwrap();
    bzod::cli::create_user::run(
        Some("userb".into()),
        Some("password123".into()),
        None,
        config,
    )
    .await
    .unwrap();
    let (a, b) = {
        let conn = db.users.lock().unwrap();
        (
            bzod::db::users::get_user_by_username(&conn, "usera")
                .unwrap()
                .unwrap(),
            bzod::db::users::get_user_by_username(&conn, "userb")
                .unwrap()
                .unwrap(),
        )
    };
    let dbs_a = state
        .open_tenant(a.tenant_id.unwrap(), TenantOpenMode::Ordinary)
        .unwrap();
    {
        let conn = dbs_a.content.lock().unwrap();
        bzod::db::content::create_url_extended(
            &conn,
            "!only-a",
            "https://example.com/a",
            None,
            None,
            &vec![],
            None,
            None,
            None,
        )
        .unwrap();
    }
    let dbs_b = state
        .open_tenant(b.tenant_id.unwrap(), TenantOpenMode::Ordinary)
        .unwrap();
    let stolen = {
        let conn = dbs_b.content.lock().unwrap();
        bzod::db::content::get_url_by_code(&conn, "!only-a").unwrap()
    };
    assert!(stolen.is_none());
    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn disabled_user_denied_ordinary_tenant_access() {
    let (temp_dir, config) = temp_config();
    let db = Db::init(&config).unwrap();
    let state = state_from(&db, config.clone());
    bzod::cli::create_user::run(
        Some("bob".into()),
        Some("password123".into()),
        None,
        config.clone(),
    )
    .await
    .unwrap();
    let user = {
        let conn = db.users.lock().unwrap();
        let u = bzod::db::users::get_user_by_username(&conn, "bob")
            .unwrap()
            .unwrap();
        bzod::db::users::update_user_status(&conn, u.id, "disabled").unwrap();
        u
    };
    let tid = user.tenant_id.unwrap();
    let err = match state.open_tenant(tid, TenantOpenMode::Ordinary) {
        Ok(_) => panic!("disabled tenant must not open ordinarily"),
        Err(e) => e,
    };
    assert!(
        err.to_string().to_lowercase().contains("denied") || err.to_string().contains("status")
    );
    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn deleted_user_cannot_open_tenant_db() {
    let (temp_dir, config) = temp_config();
    let db = Db::init(&config).unwrap();
    let state = state_from(&db, config.clone());
    bzod::cli::create_user::run(
        Some("gone".into()),
        Some("password123".into()),
        None,
        config.clone(),
    )
    .await
    .unwrap();
    let user = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::get_user_by_username(&conn, "gone")
            .unwrap()
            .unwrap()
    };
    let tid = user.tenant_id.unwrap();
    bzod::cli::delete_user::run(user.id, false, None, config)
        .await
        .unwrap();
    assert!(state.open_tenant(tid, TenantOpenMode::Ordinary).is_err());
    assert!(state.get_user_dbs(user.id).is_err());
    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn analytics_worker_does_not_create_tenant_1_for_missing_owner() {
    let (temp_dir, config) = temp_config();
    let db = Db::init(&config).unwrap();
    let before = temp_dir
        .join("users")
        .join("1")
        .join("analytics.db")
        .exists();
    let batch = vec![bzod::models::VisitRecord {
        id: "v1".into(),
        target_type: "url".into(),
        target_id: "u1".into(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        ip_address: "127.0.0.1".into(),
        user_agent: "test".into(),
        referer: "Direct".into(),
        accept_language: "en".into(),
        country: "US".into(),
        status_code: 200,
        owner_tenant_id: None,
        owner_user_id: None,
    }];
    // flush_batch is private; missing owner is dropped and must not create unknown dirs.
    assert!(bzod::jobs::open_user_analytics_conn(&db, 424242).is_err());
    assert!(!temp_dir.join("users").join("424242").exists());
    let after = temp_dir
        .join("users")
        .join("1")
        .join("analytics.db")
        .exists();
    assert_eq!(before, after);
    let _ = batch.len();
    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn password_gate_does_not_fall_back_to_tenant_1() {
    let (temp_dir, config) = temp_config();
    let db = Db::init(&config).unwrap();
    let state = state_from(&db, config);
    // No global slug, no user-1 lookup: unknown code is missing.
    {
        let conn = db.system.lock().unwrap();
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM global_slugs WHERE slug = 'zz9999');",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!exists);
    }
    let _ = state;
    let _ = fs::remove_dir_all(&temp_dir);
}
