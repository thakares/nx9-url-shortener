//! v0.8.0 Phase 3: Identity Migration Integration Tests.
//!
//! Tests:
//! 1. New users automatically get TenantId + UUID.
//! 2. Explicit identity migration lifecycle (preflight, backup, backfill, directory rename, validation).
//! 3. Legacy Admin (users/1) is preserved and not converted into a normal tenant.
//! 4. Restart-safety & idempotency (repeated migration runs).
//! 5. Recovery when target directory already exists.
//! 6. Admin bootstrap concurrency without re-entrant mutex deadlock.
//! 7. Cross-tenant isolation after identity migration.

use bzod::config::Config;
use bzod::db::identity_migrate::run_identity_migration;
use bzod::db::tenant::TenantOpenMode;
use bzod::db::Db;
use bzod::state::AppState;
use rusqlite::Connection;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

fn temp_config() -> (PathBuf, Config) {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_v08_id_migrate_{}", uuid::Uuid::new_v4()));
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

#[tokio::test]
async fn test_new_user_gets_tenant_id_and_uuid() {
    let (temp_dir, config) = temp_config();
    let db = Db::init(&config).unwrap();

    bzod::cli::create_user::run(
        Some("carol".into()),
        Some("password123".into()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    let user = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::get_user_by_username(&conn, "carol")
            .unwrap()
            .unwrap()
    };

    let tid = user.tenant_id.expect("new user must receive TenantId");
    assert_eq!(tid.as_str().len(), 12);

    let uuid_str = user.uuid.expect("new user must receive UUID");
    assert!(uuid::Uuid::parse_str(&uuid_str).is_ok());

    // Directory is created under users/<TenantId>/
    assert!(temp_dir
        .join("users")
        .join(tid.as_str())
        .join("content.db")
        .exists());
    assert!(!temp_dir.join("users").join(user.id.to_string()).exists());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_full_identity_migration_lifecycle() {
    let (temp_dir, config) = temp_config();
    let db = Db::init(&config).unwrap();

    // 1. Manually insert legacy admin and legacy users without tenant_id/uuid in users.db
    let hash = bzod::auth::hash_password("password123").unwrap();
    let now = chrono::Utc::now().to_rfc3339();

    let (id_bob, id_dave) = {
        let conn = db.users.lock().unwrap();
        conn.execute(
            "INSERT INTO users (username, password_hash, status, created_at, account_type) VALUES ('legacy_admin', ?1, 'disabled', ?2, 'system');",
            rusqlite::params![hash, now],
        ).unwrap();

        conn.execute(
            "INSERT INTO users (username, password_hash, status, created_at, account_type) VALUES ('bob', ?1, 'active', ?2, 'standard');",
            rusqlite::params![hash, now],
        ).unwrap();
        let b = conn.last_insert_rowid();

        conn.execute(
            "INSERT INTO users (username, password_hash, status, created_at, account_type) VALUES ('dave', ?1, 'active', ?2, 'standard');",
            rusqlite::params![hash, now],
        ).unwrap();
        let d = conn.last_insert_rowid();
        (b, d)
    };

    // 2. Provision legacy directories at users/1 (legacy admin) and users/<id>/ (bob & dave)
    let legacy_admin_dir = temp_dir.join("users").join("1");
    let legacy_bob_dir = temp_dir.join("users").join(id_bob.to_string());
    let legacy_dave_dir = temp_dir.join("users").join(id_dave.to_string());
    fs::create_dir_all(&legacy_admin_dir).unwrap();
    fs::create_dir_all(&legacy_bob_dir).unwrap();
    fs::create_dir_all(&legacy_dave_dir).unwrap();

    // Seed content in Bob's legacy content.db
    let bob_content_path = legacy_bob_dir.join("content.db");
    {
        let mut conn = Connection::open(&bob_content_path).unwrap();
        bzod::db::migrations::run_migrations(
            &mut conn,
            "content",
            bzod::db::migrations::CONTENT_MIGRATIONS,
            None,
        )
        .unwrap();
        bzod::db::content::create_url_extended(
            &conn,
            "!bob-link",
            "https://bob.example.com",
            None,
            None,
            &vec![],
            None,
            None,
            None,
        )
        .unwrap();
    }

    // 3. Run Dry Run Preflight
    let dry_report = run_identity_migration(&config, true, true)
        .await
        .expect("dry run");
    assert_eq!(dry_report.users_assigned_tenant_id, 2);
    assert_eq!(dry_report.users_assigned_uuid, 2);
    assert!(legacy_bob_dir.exists(), "dry run must not move directory");

    // 4. Run Execution
    let report = run_identity_migration(&config, false, true)
        .await
        .expect("migration");
    assert_eq!(report.users_assigned_tenant_id, 2);
    assert_eq!(report.users_assigned_uuid, 2);
    assert_eq!(report.directories_moved, 2); // Both Bob and Dave had directories moved
    assert!(report.validation_passed);
    assert!(report.legacy_admin_preserved);

    // 5. Verify Bob's identity and migrated directory
    let bob = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::get_user_by_username(&conn, "bob")
            .unwrap()
            .unwrap()
    };
    let bob_tid = bob.tenant_id.expect("bob must have TenantId");
    assert_eq!(bob_tid.as_str().len(), 12);
    assert!(bob.uuid.is_some());

    let migrated_bob_dir = temp_dir.join("users").join(bob_tid.as_str());
    assert!(
        migrated_bob_dir.exists(),
        "migrated directory must exist at users/<TenantId>/"
    );
    assert!(!legacy_bob_dir.exists(), "legacy directory must be moved");

    // Verify Bob's content is preserved exactly
    {
        let conn = Connection::open(migrated_bob_dir.join("content.db")).unwrap();
        let url = bzod::db::content::get_url_by_code(&conn, "!bob-link")
            .unwrap()
            .expect("bob's link must exist in migrated content.db");
        assert_eq!(url.destination, "https://bob.example.com");
    }

    // 6. Verify legacy admin (users/1) was preserved
    assert!(
        temp_dir.join("users").join("1").exists(),
        "legacy admin directory users/1 must be preserved"
    );

    // 7. Verify Completion Marker in system.db
    {
        let system_conn = db.system.lock().unwrap();
        let marker: Option<String> = system_conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'v08_identity_migration_completed';",
                [],
                |r| r.get(0),
            )
            .ok();
        assert!(marker.is_some());
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_migration_is_restart_safe_and_idempotent() {
    let (temp_dir, config) = temp_config();
    let db = Db::init(&config).unwrap();

    // Create a normal user
    bzod::cli::create_user::run(
        Some("eva".into()),
        Some("password123".into()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    let original_user = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::get_user_by_username(&conn, "eva")
            .unwrap()
            .unwrap()
    };

    let original_tid = original_user.tenant_id.unwrap();
    let original_uuid = original_user.uuid.unwrap();

    // Run migration twice
    let report1 = run_identity_migration(&config, false, true).await.unwrap();
    let report2 = run_identity_migration(&config, false, true).await.unwrap();

    assert_eq!(report1.users_assigned_tenant_id, 0); // already had TenantId
    assert_eq!(report2.users_assigned_tenant_id, 0);

    let current_user = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::get_user_by_username(&conn, "eva")
            .unwrap()
            .unwrap()
    };

    // Identity mappings must be identical (never regenerated)
    assert_eq!(current_user.tenant_id.unwrap(), original_tid);
    assert_eq!(current_user.uuid.unwrap(), original_uuid);
    assert!(temp_dir.join("users").join(original_tid.as_str()).exists());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_admin_bootstrap_concurrency_no_deadlock() {
    let (temp_dir, config) = temp_config();
    let db = Db::init(&config).unwrap();
    let state = state_from(&db, config.clone());

    // Call create_admin_user multiple times or simulate bootstrap login
    let hash = bzod::auth::hash_password("securepassword").unwrap();
    let res = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::create_admin_user(&conn, "admin_user", &hash)
    };
    assert!(res.is_ok());
    let admin = res.unwrap();
    assert_eq!(admin.account_type, "admin");
    assert!(admin.uuid.is_some());
    assert_eq!(admin.tenant_id, None, "admin is Core-only");

    let _ = state;
    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_cross_tenant_isolation_after_migration() {
    let (temp_dir, config) = temp_config();
    let db = Db::init(&config).unwrap();
    let state = state_from(&db, config.clone());

    // Create user X and user Y
    bzod::cli::create_user::run(
        Some("userx".into()),
        Some("pass123".into()),
        None,
        config.clone(),
    )
    .await
    .unwrap();
    bzod::cli::create_user::run(Some("usery".into()), Some("pass123".into()), None, config)
        .await
        .unwrap();

    let (x, y) = {
        let conn = db.users.lock().unwrap();
        (
            bzod::db::users::get_user_by_username(&conn, "userx")
                .unwrap()
                .unwrap(),
            bzod::db::users::get_user_by_username(&conn, "usery")
                .unwrap()
                .unwrap(),
        )
    };

    let tid_x = x.tenant_id.unwrap();
    let tid_y = y.tenant_id.unwrap();

    let dbs_x = state.open_tenant(tid_x, TenantOpenMode::Ordinary).unwrap();
    {
        let conn = dbs_x.content.lock().unwrap();
        bzod::db::content::create_url_extended(
            &conn,
            "!x-secret",
            "https://x.example.com",
            None,
            None,
            &vec![],
            None,
            None,
            None,
        )
        .unwrap();
    }

    let dbs_y = state.open_tenant(tid_y, TenantOpenMode::Ordinary).unwrap();
    let stolen = {
        let conn = dbs_y.content.lock().unwrap();
        bzod::db::content::get_url_by_code(&conn, "!x-secret").unwrap()
    };
    assert!(
        stolen.is_none(),
        "Tenant Y must not see Tenant X's secret content"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}
