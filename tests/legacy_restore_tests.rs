//! Tests for legacy_flat_backup restore compatibility and current backup roundtrip.
//!
//! These tests use a synthetic fixture that reproduces the exact structure of
//! a real legacy_flat_backup archive:
//!   - admin.db with a users table (TEXT UUID PK, username, password_hash)
//!   - users.db that is completely empty (no tables, user_version=0)
//!   - system.db with global_slugs referencing multiple owner_user_ids
//!   - content.db with URLs and landing pages
//!   - analytics.db with visits
//!   - backup_manifest.json with type "legacy_flat_backup"
//!   - Orphaned slug entries (in global_slugs but not in content.db)
//!   - A missing tenant (owner_user_id=3 whose databases are not included)

use bzod::config::Config;
use bzod::db::Db;
use flate2::write::GzEncoder;
use flate2::Compression;
use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;
use tar::Builder;

fn create_temp_config(temp_dir: PathBuf) -> Config {
    let mut config = Config::load();
    config.data_dir = temp_dir.clone();
    config.backup_dir = temp_dir.clone();
    config.base_url = Some("http://bzo.in".to_string());
    config
}

/// Build a synthetic legacy_flat_backup .tar.gz archive that reproduces the
/// exact structure of the real production backup that fails with:
///   "Failed to verify registry integrity in backup: no such table: users"
///
/// IMPORTANT: This uses purely synthetic data — no real credentials, URLs,
/// audit logs, or analytics from the production backup are included.
fn build_synthetic_legacy_fixture(output_path: &std::path::Path) {
    use chrono::Utc;
    let fixture_dir =
        std::env::temp_dir().join(format!("bzod_fixture_build_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&fixture_dir).unwrap();

    let now = Utc::now().to_rfc3339();

    // --- admin.db: legacy admin schema with TEXT UUID primary key ---
    {
        let conn = Connection::open(fixture_dir.join("admin.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE users (
                id TEXT PRIMARY KEY,
                username TEXT NOT NULL UNIQUE,
                password_hash TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE api_keys (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                key_hash TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL,
                created_at TEXT NOT NULL,
                last_used_at TEXT
            );
            CREATE TABLE audit_logs (
                id TEXT PRIMARY KEY,
                timestamp TEXT NOT NULL,
                username TEXT NOT NULL,
                action TEXT NOT NULL,
                object_type TEXT,
                object_id TEXT,
                ip_address TEXT,
                user_agent TEXT
            );
            CREATE TABLE config (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )
        .unwrap();

        // Insert a synthetic admin with a known argon2id hash
        // (this is NOT a real password hash — it's a valid format placeholder)
        let admin_hash =
            "$argon2id$v=19$m=19456,t=2,p=1$dGVzdHNhbHQ$syntheticHashForTestingOnly00000000000000";
        conn.execute(
            "INSERT INTO users (id, username, password_hash, created_at) VALUES (?1, ?2, ?3, ?4);",
            rusqlite::params![
                "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
                "admin",
                admin_hash,
                &now
            ],
        )
        .unwrap();

        // Insert an audit log entry
        conn.execute(
            "INSERT INTO audit_logs (id, timestamp, username, action) VALUES (?1, ?2, ?3, ?4);",
            rusqlite::params!["audit-1", &now, "admin", "LOGIN"],
        )
        .unwrap();

        // Set user_version to 1 (matching legacy migration state)
        conn.execute_batch("PRAGMA user_version = 1;").unwrap();
    }

    // --- system.db: has global_slugs with multiple owners + orphaned entries ---
    {
        let mut conn = Connection::open(fixture_dir.join("system.db")).unwrap();
        // Run system migrations to get the full schema
        bzod::db::migrations::run_migrations(
            &mut conn,
            "system",
            bzod::db::migrations::SYSTEM_MIGRATIONS,
            None,
        )
        .unwrap();

        // Insert global_slugs owned by user_id=1 (content exists)
        for (slug, target_type, target_id) in &[
            ("abc123", "url", "url-id-1"),
            ("def456", "url", "url-id-2"),
            ("!custom-slug", "url", "url-id-3"),
            ("!test-page", "page", "page-id-1"),
            ("!meeting", "page", "page-id-2"),
        ] {
            conn.execute(
                "INSERT INTO global_slugs (slug, owner_user_id, target_type, target_id, created_at, updated_at, status)
                 VALUES (?1, 1, ?2, ?3, ?4, ?5, 'active');",
                rusqlite::params![slug, target_type, target_id, &now, &now],
            )
            .unwrap();
        }

        // Insert global_slugs owned by user_id=3 (tenant NOT included in flat backup)
        for (slug, target_type, target_id) in &[
            ("xyz789", "url", "user3-url-1"),
            ("!user3-page", "page", "user3-page-1"),
        ] {
            conn.execute(
                "INSERT INTO global_slugs (slug, owner_user_id, target_type, target_id, created_at, updated_at, status)
                 VALUES (?1, 3, ?2, ?3, ?4, ?5, 'active');",
                rusqlite::params![slug, target_type, target_id, &now, &now],
            )
            .unwrap();
        }

        // Insert an orphaned slug (user_id=1, content doesn't exist)
        conn.execute(
            "INSERT INTO global_slugs (slug, owner_user_id, target_type, target_id, created_at, updated_at, status)
             VALUES ('orphan-slug', 1, 'url', 'nonexistent-id', ?1, ?2, 'active');",
            rusqlite::params![&now, &now],
        )
        .unwrap();
    }

    // --- users.db: completely empty (no schema, user_version=0) ---
    {
        let _conn = Connection::open(fixture_dir.join("users.db")).unwrap();
        // Intentionally empty — this is the root cause of the original bug
    }

    // --- content.db: URLs and landing pages belonging to user_id=1 ---
    {
        let mut conn = Connection::open(fixture_dir.join("content.db")).unwrap();
        bzod::db::migrations::run_migrations(
            &mut conn,
            "content",
            bzod::db::migrations::CONTENT_MIGRATIONS,
            None,
        )
        .unwrap();

        // Insert URLs
        for (id, code, dest) in &[
            ("url-id-1", "abc123", "https://example.com/1"),
            ("url-id-2", "def456", "https://example.com/2"),
            ("url-id-3", "!custom-slug", "https://example.com/3"),
        ] {
            conn.execute(
                "INSERT INTO urls (id, code, destination, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'active', ?4, ?5);",
                rusqlite::params![id, code, dest, &now, &now],
            )
            .unwrap();
        }

        // Insert landing pages
        for (id, code, title) in &[
            ("page-id-1", "!test-page", "Test Page"),
            ("page-id-2", "!meeting", "Meeting Notes"),
        ] {
            conn.execute(
                "INSERT INTO landing_pages (id, code, slug, title, html_content, state, created_at, updated_at)
                 VALUES (?1, ?2, ?2, ?3, '<h1>Test</h1>', 'published', ?4, ?5);",
                rusqlite::params![id, code, title, &now, &now],
            )
            .unwrap();
        }
    }

    // --- analytics.db: visits ---
    {
        let mut conn = Connection::open(fixture_dir.join("analytics.db")).unwrap();
        bzod::db::migrations::run_migrations(
            &mut conn,
            "analytics",
            bzod::db::migrations::ANALYTICS_MIGRATIONS,
            None,
        )
        .unwrap();

        // Insert some visits
        for i in 0..10 {
            conn.execute(
                "INSERT INTO visits (id, target_type, target_id, timestamp, owner_user_id, ip_address, user_agent, referer, accept_language, country, status_code)
                 VALUES (?1, 'url', 'url-id-1', ?2, 1, ?3, ?4, ?5, ?6, ?7, ?8);",
                rusqlite::params![format!("visit-{}", i), &now, "127.0.0.1", "test-agent", "", "en-US", "US", 200],
            )
            .unwrap();
        }
    }

    // --- backup_manifest.json ---
    let manifest = serde_json::json!({
        "created_at": &now,
        "type": "legacy_flat_backup",
        "files_included": ["admin.db", "system.db", "users.db", "content.db", "analytics.db"],
        "note": "Multi-tenant databases flattened for backward compatibility.",
    });
    fs::write(
        fixture_dir.join("backup_manifest.json"),
        manifest.to_string(),
    )
    .unwrap();

    // --- Package into .tar.gz ---
    let tar_file = fs::File::create(output_path).unwrap();
    let enc = GzEncoder::new(tar_file, Compression::default());
    let mut tar = Builder::new(enc);

    for name in &[
        "admin.db",
        "system.db",
        "users.db",
        "content.db",
        "analytics.db",
        "backup_manifest.json",
    ] {
        tar.append_path_with_name(fixture_dir.join(name), name)
            .unwrap();
    }

    tar.into_inner().unwrap().finish().unwrap();
    let _ = fs::remove_dir_all(&fixture_dir);
}

// ==========================================================================
// Test 1: Legacy flat backup restores without "no such table" error
// ==========================================================================
#[test]
fn test_legacy_flat_backup_restore() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_legacy_restore_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();

    let fixture_path = temp_dir.join("legacy-backup.tar.gz");
    build_synthetic_legacy_fixture(&fixture_path);

    let restore_dir = temp_dir.join("restored_data");
    fs::create_dir_all(&restore_dir).unwrap();

    // This must succeed — previously it failed with "no such table: users"
    let result = bzod::cli::restore::perform_restore(&fixture_path, &restore_dir);
    assert!(
        result.is_ok(),
        "Legacy flat backup restore failed: {:?}",
        result.err()
    );

    // Verify multi-tenant directory structure
    assert!(
        restore_dir.join("admin/admin.db").exists(),
        "admin/admin.db missing"
    );
    assert!(
        restore_dir.join("admin/system.db").exists(),
        "admin/system.db missing"
    );
    assert!(
        restore_dir.join("admin/users.db").exists(),
        "admin/users.db missing"
    );
    assert!(
        restore_dir.join("users/1/content.db").exists(),
        "users/1/content.db missing"
    );
    assert!(
        restore_dir.join("users/1/analytics.db").exists(),
        "users/1/analytics.db missing"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

// ==========================================================================
// Test 2: Admin credentials are preserved, not manufactured
// ==========================================================================
#[test]
fn test_legacy_restore_preserves_admin_credentials() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_legacy_creds_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();

    let fixture_path = temp_dir.join("legacy-backup.tar.gz");
    build_synthetic_legacy_fixture(&fixture_path);

    let restore_dir = temp_dir.join("restored_data");
    fs::create_dir_all(&restore_dir).unwrap();

    bzod::cli::restore::perform_restore(&fixture_path, &restore_dir).unwrap();

    let expected_hash =
        "$argon2id$v=19$m=19456,t=2,p=1$dGVzdHNhbHQ$syntheticHashForTestingOnly00000000000000";

    // Verify original admin identity in admin.db is untouched
    {
        let conn = Connection::open(restore_dir.join("admin/admin.db")).unwrap();
        let (username, hash): (String, String) = conn
            .query_row(
                "SELECT username, password_hash FROM users WHERE id = 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee';",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(username, "admin");
        assert_eq!(hash, expected_hash, "Admin password hash was modified!");
    }

    // Verify users.db was bootstrapped with actual admin credentials
    {
        let conn = Connection::open(restore_dir.join("admin/users.db")).unwrap();

        // legacy_admin system placeholder should exist with id=1
        let (la_username, la_hash, la_type): (String, String, String) = conn
            .query_row(
                "SELECT username, password_hash, account_type FROM users WHERE id = 1;",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(la_username, "legacy_admin");
        assert_eq!(
            la_hash, expected_hash,
            "legacy_admin hash should match original admin"
        );
        assert_eq!(la_type, "system");

        // Actual admin account should exist with original credentials
        let (admin_hash, admin_type, admin_status): (String, String, String) = conn
            .query_row(
                "SELECT password_hash, account_type, status FROM users WHERE username = 'admin';",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            admin_hash, expected_hash,
            "Admin account hash should match original"
        );
        assert_eq!(admin_type, "admin");
        assert_eq!(admin_status, "active");
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

// ==========================================================================
// Test 3: Functional data is preserved and accessible after restore + Db::init()
// ==========================================================================
#[tokio::test]
async fn test_legacy_restore_functional_data() {
    let temp_dir = std::env::temp_dir().join(format!(
        "bzod_test_legacy_functional_{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&temp_dir).unwrap();

    let fixture_path = temp_dir.join("legacy-backup.tar.gz");
    build_synthetic_legacy_fixture(&fixture_path);

    let restore_dir = temp_dir.join("restored_data");
    fs::create_dir_all(&restore_dir).unwrap();

    bzod::cli::restore::perform_restore(&fixture_path, &restore_dir).unwrap();

    // Initialize Db against the restored data (simulates fresh v0.6.0 startup)
    let config = create_temp_config(restore_dir.clone());
    let db = Db::init(&config).expect("Db::init failed on restored legacy data");

    // Verify URLs
    {
        let content_conn = bzod::jobs::open_user_content_conn(&db, 1).unwrap();
        let url_count: i64 = content_conn
            .query_row("SELECT COUNT(*) FROM urls;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(url_count, 3, "Expected 3 URLs in restored content.db");

        let url = bzod::db::content::get_url_by_code(&content_conn, "abc123")
            .unwrap()
            .unwrap();
        assert_eq!(url.destination, "https://example.com/1");
    }

    // Verify landing pages
    {
        let content_conn = bzod::jobs::open_user_content_conn(&db, 1).unwrap();
        let page_count: i64 = content_conn
            .query_row("SELECT COUNT(*) FROM landing_pages;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            page_count, 2,
            "Expected 2 landing pages in restored content.db"
        );
    }

    // Verify analytics
    {
        let analytics_conn = bzod::jobs::open_user_analytics_conn(&db, 1).unwrap();
        let visit_count: i64 = analytics_conn
            .query_row("SELECT COUNT(*) FROM visits;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            visit_count, 10,
            "Expected 10 visits in restored analytics.db"
        );
    }

    // Verify global slug registry
    {
        let system_conn = db.system.lock().unwrap();
        let slug_count: i64 = system_conn
            .query_row("SELECT COUNT(*) FROM global_slugs;", [], |r| r.get(0))
            .unwrap();
        assert!(
            slug_count >= 7,
            "Expected at least 7 global_slugs (5 user1 + 2 user3 + orphan)"
        );
    }

    // Verify user 3 placeholder exists
    {
        let users_conn = db.users.lock().unwrap();
        let user3_exists: bool = users_conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM users WHERE id = 3);",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            user3_exists,
            "Placeholder for user_id=3 should exist in users.db"
        );

        let (status, metadata): (String, Option<String>) = users_conn
            .query_row(
                "SELECT status, metadata FROM users WHERE id = 3;",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "disabled", "User 3 placeholder should be disabled");
        assert!(
            metadata.as_deref().unwrap_or("").contains("Placeholder"),
            "User 3 metadata should document it as a placeholder"
        );
    }

    // Verify admin identity in admin.db
    {
        let admin_conn = db.admin.lock().unwrap();
        let admin_exists: bool = admin_conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM users WHERE username = 'admin');",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            admin_exists,
            "Original admin identity must be preserved in admin.db"
        );
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

// ==========================================================================
// Test 4: Current/native backup restore roundtrip
// ==========================================================================
#[tokio::test]
async fn test_current_backup_restore_roundtrip() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_current_rt_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");

    // Create a user and add content
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

    {
        let user_content_conn = bzod::jobs::open_user_content_conn(&db, user_id).unwrap();
        bzod::db::content::create_url_extended(
            &user_content_conn,
            "!current-test",
            "https://example.com/current",
            None,
            None,
            &vec![],
            None,
            None,
            None,
        )
        .unwrap();

        let system_conn = db.system.lock().unwrap();
        bzod::db::users::register_global_slug(
            &system_conn,
            "!current-test",
            user_id,
            "url",
            "current-id",
            "active",
        )
        .unwrap();
    }

    // Create a native backup
    let backup_path = bzod::jobs::backup::perform_backup(&db, &config)
        .await
        .unwrap();

    // Restore to a fresh directory
    let restore_dir = temp_dir.join("restored_native");
    fs::create_dir_all(&restore_dir).unwrap();

    bzod::cli::restore::perform_restore(std::path::Path::new(&backup_path), &restore_dir).unwrap();

    // Verify restored data
    let restore_config = create_temp_config(restore_dir.clone());
    let restored_db = Db::init(&restore_config).expect("Db::init failed on restored native data");

    {
        let conn = restored_db.users.lock().unwrap();
        let user = bzod::db::users::get_user_by_username(&conn, "testuser")
            .unwrap()
            .unwrap();
        assert_eq!(user.status, "active");
    }

    {
        let content_conn = bzod::jobs::open_user_content_conn(&restored_db, user_id).unwrap();
        let url = bzod::db::content::get_url_by_code(&content_conn, "!current-test")
            .unwrap()
            .unwrap();
        assert_eq!(url.destination, "https://example.com/current");
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

// ==========================================================================
// Test 5: Failed restore does not corrupt existing data
// ==========================================================================
#[tokio::test]
async fn test_failed_restore_does_not_corrupt() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_safe_restore_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    // Set up a working installation
    let _db = Db::init(&config).expect("Failed to init Db");

    // Write a sentinel file to verify the data dir survives
    let sentinel = config.data_dir.join("admin").join("sentinel.txt");
    fs::write(&sentinel, "intact").unwrap();

    // Create a corrupt "backup" file
    let corrupt_path = temp_dir.join("corrupt.tar.gz");
    fs::write(&corrupt_path, b"this is not a valid tar.gz file").unwrap();

    // Attempt restore — must fail
    let result = bzod::cli::restore::perform_restore(&corrupt_path, &config.data_dir);
    assert!(result.is_err(), "Corrupt backup should fail to restore");

    // Verify original data is intact
    assert!(
        sentinel.exists(),
        "Sentinel file must survive failed restore"
    );
    assert_eq!(
        fs::read_to_string(&sentinel).unwrap(),
        "intact",
        "Sentinel file content must be unchanged"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}
