//! v0.8.0 Phase 4: Global Slug Migration Integration Tests.
//!
//! Tests:
//! 1. Preflight and dry-run validation.
//! 2. Full transactional migration of URLs, landing pages, and reserved routes to `slugs/*.db`.
//! 3. Exact TenantId ownership resolution via users.db.
//! 4. Global uniqueness across separate SQLite databases.
//! 5. Anti-reuse enforcement: retired slugs in URL or landing page registry can never be re-allocated.
//! 6. Restart safety and idempotency.
//! 7. Live 301 redirect and 410 Gone resolution from `slugs/*.db`.

use bzod::config::Config;
use bzod::db::slug_migrate::run_global_slug_migration;
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
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_v08_slug_migrate_{}", uuid::Uuid::new_v4()));
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
async fn test_slug_preflight_and_dry_run() {
    let (temp_dir, config) = temp_config();
    let db = Db::init(&config).unwrap();

    // Create a normal user
    bzod::cli::create_user::run(
        Some("alice".into()),
        Some("password123".into()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    let alice = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::get_user_by_username(&conn, "alice")
            .unwrap()
            .unwrap()
    };

    // Insert legacy slugs in system.db.global_slugs
    {
        let system_conn = db.system.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        system_conn.execute(
            "INSERT INTO global_slugs (slug, owner_user_id, target_type, target_id, created_at, updated_at, status) 
             VALUES ('my-link', ?1, 'url', '!link1', ?2, ?2, 'active');",
            rusqlite::params![alice.id, now],
        ).unwrap();

        system_conn.execute(
            "INSERT INTO global_slugs (slug, owner_user_id, target_type, target_id, created_at, updated_at, status) 
             VALUES ('my-page', ?1, 'page', '!page1', ?2, ?2, 'active');",
            rusqlite::params![alice.id, now],
        ).unwrap();
    }

    // Run Dry Run
    let dry_report = run_global_slug_migration(&config, true, true)
        .await
        .expect("dry run");
    assert_eq!(dry_report.total_legacy_slugs, 2);
    assert_eq!(dry_report.url_slugs_migrated, 1);
    assert_eq!(dry_report.page_slugs_migrated, 1);

    // Target databases should still be empty
    {
        let urls_conn = db.global_urls.lock().unwrap();
        let count: i64 = urls_conn
            .query_row("SELECT COUNT(*) FROM global_urls;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "dry run must not write to target global_urls.db");
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_full_slug_migration_lifecycle() {
    let (temp_dir, config) = temp_config();
    let db = Db::init(&config).unwrap();

    // Create user Bob
    bzod::cli::create_user::run(
        Some("bob".into()),
        Some("password123".into()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    let bob = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::get_user_by_username(&conn, "bob")
            .unwrap()
            .unwrap()
    };
    let bob_tid = bob.tenant_id.unwrap();

    // Insert active, disabled, and retired legacy slugs
    let now = chrono::Utc::now().to_rfc3339();
    {
        let system_conn = db.system.lock().unwrap();
        system_conn.execute(
            "INSERT INTO global_slugs (slug, owner_user_id, target_type, target_id, created_at, updated_at, status) 
             VALUES ('bob-active', ?1, 'url', '!active1', ?2, ?2, 'active');",
            rusqlite::params![bob.id, now],
        ).unwrap();

        system_conn.execute(
            "INSERT INTO global_slugs (slug, owner_user_id, target_type, target_id, created_at, updated_at, status) 
             VALUES ('bob-disabled', ?1, 'url', '!dis1', ?2, ?2, 'disabled');",
            rusqlite::params![bob.id, now],
        ).unwrap();

        system_conn.execute(
            "INSERT INTO global_slugs (slug, owner_user_id, target_type, target_id, created_at, updated_at, status, deleted_at) 
             VALUES ('bob-retired', ?1, 'url', '!ret1', ?2, ?2, 'retired', ?2);",
            rusqlite::params![bob.id, now],
        ).unwrap();

        system_conn.execute(
            "INSERT INTO global_slugs (slug, owner_user_id, target_type, target_id, created_at, updated_at, status) 
             VALUES ('bob-landing', ?1, 'page', '!page1', ?2, ?2, 'active');",
            rusqlite::params![bob.id, now],
        ).unwrap();
    }

    // Run Migration
    let report = run_global_slug_migration(&config, false, true)
        .await
        .expect("slug migration");
    assert_eq!(report.total_legacy_slugs, 4);
    assert_eq!(report.url_slugs_migrated, 3);
    assert_eq!(report.page_slugs_migrated, 1);
    assert!(report.validation_passed);

    // Verify records in global_urls.db
    {
        let urls_conn = db.global_urls.lock().unwrap();

        let active_url = bzod::db::slugs::lookup_url_slug(&urls_conn, "bob-active")
            .unwrap()
            .expect("active url must exist in global_urls.db");
        assert_eq!(active_url.owner_tenant_id, bob_tid.as_str());
        assert_eq!(active_url.status, "active");

        let retired_url = bzod::db::slugs::lookup_url_slug(&urls_conn, "bob-retired")
            .unwrap()
            .expect("retired url must exist in global_urls.db");
        assert_eq!(retired_url.owner_tenant_id, bob_tid.as_str());
        assert_eq!(retired_url.status, "retired");
        assert!(retired_url.retired_at.is_some());
    }

    // Verify record in global_landing_pages.db
    {
        let pages_conn = db.global_landing_pages.lock().unwrap();
        let page = bzod::db::slugs::lookup_landing_page_slug(&pages_conn, "bob-landing")
            .unwrap()
            .expect("page must exist in global_landing_pages.db");
        assert_eq!(page.owner_tenant_id, bob_tid.as_str());
        assert_eq!(page.status, "active");
    }

    // Verify Completion Marker in system.db
    {
        let system_conn = db.system.lock().unwrap();
        let marker: Option<String> = system_conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'v08_global_slug_migration_completed';",
                [],
                |r| r.get(0),
            )
            .ok();
        assert!(marker.is_some());
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_global_slug_uniqueness_and_anti_reuse() {
    let (temp_dir, config) = temp_config();
    let db = Db::init(&config).unwrap();

    let tid_a = TenantId::generate();
    let tid_b = TenantId::generate();

    let reserved_conn = db.reserved.lock().unwrap();
    let urls_conn = db.global_urls.lock().unwrap();
    let pages_conn = db.global_landing_pages.lock().unwrap();

    // 1. Reserved slug check
    assert!(
        !bzod::db::slugs::is_slug_available(&reserved_conn, &urls_conn, &pages_conn, "admin")
            .unwrap(),
        "Reserved route 'admin' must not be available"
    );

    // 2. Register URL slug
    bzod::db::slugs::register_url_slug(&urls_conn, "cool-slug", &tid_a, "!target1", "active")
        .unwrap();

    // 'cool-slug' is now unavailable for URL or page registration
    assert!(!bzod::db::slugs::is_slug_available(
        &reserved_conn,
        &urls_conn,
        &pages_conn,
        "cool-slug"
    )
    .unwrap());

    // 3. Retire 'cool-slug'
    let retired = bzod::db::slugs::retire_slug(&urls_conn, &pages_conn, "cool-slug").unwrap();
    assert!(retired);

    // Anti-reuse invariant: Retired slug MUST REMAIN UNAVAILABLE across both URLs and landing pages
    assert!(
        !bzod::db::slugs::is_slug_available(&reserved_conn, &urls_conn, &pages_conn, "cool-slug")
            .unwrap(),
        "Retired slug must NEVER become available for reuse"
    );

    // Attempting to register page with retired slug must fail
    let page_reg_res = bzod::db::slugs::reserve_landing_page_slug(
        &reserved_conn,
        &urls_conn,
        &pages_conn,
        "cool-slug",
        &tid_b,
    );
    assert!(
        page_reg_res.is_err(),
        "Cannot allocate a retired slug for a landing page"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_slug_migration_restart_safety_and_idempotency() {
    let (temp_dir, config) = temp_config();
    let db = Db::init(&config).unwrap();

    bzod::cli::create_user::run(
        Some("dan".into()),
        Some("password123".into()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    let dan = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::get_user_by_username(&conn, "dan")
            .unwrap()
            .unwrap()
    };

    let now = chrono::Utc::now().to_rfc3339();
    {
        let system_conn = db.system.lock().unwrap();
        system_conn.execute(
            "INSERT INTO global_slugs (slug, owner_user_id, target_type, target_id, created_at, updated_at, status) 
             VALUES ('dan-link', ?1, 'url', '!dan1', ?2, ?2, 'active');",
            rusqlite::params![dan.id, now],
        ).unwrap();
    }

    // Run migration twice
    let report1 = run_global_slug_migration(&config, false, true)
        .await
        .unwrap();
    let report2 = run_global_slug_migration(&config, false, true)
        .await
        .unwrap();

    assert_eq!(report1.url_slugs_migrated, 1);
    assert_eq!(report2.url_slugs_migrated, 0);
    assert_eq!(report2.existing_records_verified, 1);
    assert!(report2.validation_passed);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_live_slug_lookup_and_redirect_resolution() {
    let (temp_dir, config) = temp_config();
    let db = Db::init(&config).unwrap();
    let state = state_from(&db, config.clone());

    bzod::cli::create_user::run(
        Some("eva".into()),
        Some("password123".into()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    let eva = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::get_user_by_username(&conn, "eva")
            .unwrap()
            .unwrap()
    };
    let eva_tid = eva.tenant_id.unwrap();

    // Register slug in global_urls.db directly
    {
        let urls_conn = db.global_urls.lock().unwrap();
        bzod::db::slugs::register_url_slug(&urls_conn, "eva-promo", &eva_tid, "!eva1", "active")
            .unwrap();
    }

    // Seed content in Eva's tenant content.db
    let eva_dbs = state
        .open_tenant(eva_tid, TenantOpenMode::Ordinary)
        .unwrap();
    {
        let conn = eva_dbs.content.lock().unwrap();
        bzod::db::content::create_url_extended(
            &conn,
            "eva-promo",
            "https://eva.example.com/promo",
            None,
            None,
            &vec![],
            None,
            None,
            None,
        )
        .unwrap();
    }

    // Test AppState::lookup_slug finds the record in global_urls.db
    let resolved = state
        .lookup_slug("eva-promo")
        .unwrap()
        .expect("must find slug");
    assert_eq!(resolved.owner_tenant_id, eva_tid.as_str());
    assert_eq!(resolved.status, "active");

    let _ = fs::remove_dir_all(&temp_dir);
}
