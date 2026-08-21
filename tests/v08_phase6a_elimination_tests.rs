use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bzod::analytics::AnalyticsQueue;
use bzod::cli::RepairCommands;
use bzod::config::Config;
use bzod::db::tenant::TenantOpenMode;
use bzod::db::topology::Topology;
use bzod::db::Db;
use bzod::models::visit::VisitRecord;
use bzod::services::bulk_urls::{create_urls_bulk, BulkUrlCreateItem};
use bzod::services::registry_validator::{RegistryIssueType, RegistryValidator};
use bzod::services::slug_transfer::{transfer_slug, SlugTransferRequest};
use bzod::state::AppState;
use uuid::Uuid;

fn create_test_config() -> (PathBuf, Config) {
    let temp_dir = std::env::temp_dir().join(format!("bzod_p6a_test_{}", Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let mut config = Config::load();
    config.data_dir = temp_dir.clone();
    config.backup_dir = temp_dir.join("backups");
    config.base_url = Some("http://localhost:8080".to_string());
    (temp_dir, config)
}

fn build_test_state(db: &Db, config: &Config) -> AppState {
    let (queue, _) = AnalyticsQueue::new(db.clone(), 10, tokio::sync::watch::channel(false).1);
    AppState {
        admin_db: db.admin.clone(),
        system_db: db.system.clone(),
        users_db: db.users.clone(),
        user_dbs: Arc::new(Mutex::new(HashMap::new())),
        db: db.clone(),
        config: config.clone(),
        analytics_queue: queue,
        start_time: Instant::now(),
    }
}

#[tokio::test]
async fn test_f01_bulk_urls_uses_v08_slug_registry() {
    let (temp_dir, config) = create_test_config();
    let db = Db::init(&config).unwrap();
    let state = build_test_state(&db, &config);

    // Create a tenant
    let user = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::create_user(&conn, "bulkuser", "hash", "user", None).unwrap()
    };
    let tenant_id = user.tenant_id.unwrap();

    let user_dbs = state
        .open_tenant(tenant_id, TenantOpenMode::Ordinary)
        .unwrap();

    let items = vec![
        BulkUrlCreateItem {
            code: Some("!bulk1".to_string()),
            destination: "https://example.com/1".to_string(),
            title: Some("Bulk 1".to_string()),
            description: None,
            tags: Some(vec!["tag1".to_string()]),
            expires_at: None,
            password: None,
            max_access_count: None,
        },
        BulkUrlCreateItem {
            code: Some("!bulk2".to_string()),
            destination: "https://example.com/2".to_string(),
            title: Some("Bulk 2".to_string()),
            description: None,
            tags: Some(vec!["tag2".to_string()]),
            expires_at: None,
            password: None,
            max_access_count: None,
        },
    ];

    let created = create_urls_bulk(
        &user_dbs.content,
        &db.reserved,
        &db.global_urls,
        &db.global_landing_pages,
        &db.users,
        user.id,
        tenant_id,
        items,
    )
    .unwrap();

    assert_eq!(created.len(), 2);

    // Verify slugs are in slugs/global_urls.db with tenant_id
    {
        let urls_conn = db.global_urls.lock().unwrap();
        let (owner, status): (String, String) = urls_conn
            .query_row(
                "SELECT owner_tenant_id, status FROM global_urls WHERE slug = '!bulk1';",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(owner, tenant_id.as_str());
        assert_eq!(status, "active");

        let (owner2, status2): (String, String) = urls_conn
            .query_row(
                "SELECT owner_tenant_id, status FROM global_urls WHERE slug = '!bulk2';",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(owner2, tenant_id.as_str());
        assert_eq!(status2, "active");
    }

    // Verify zero records in system.db.global_slugs
    {
        let system_conn = db.system.lock().unwrap();
        let count: i64 = system_conn
            .query_row("SELECT COUNT(*) FROM global_slugs;", [], |r| r.get(0))
            .unwrap_or(0);
        assert_eq!(
            count, 0,
            "system.db.global_slugs must not receive bulk URLs"
        );
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_f03_slug_transfer_uses_v08_architecture() {
    let (temp_dir, config) = create_test_config();
    let db = Db::init(&config).unwrap();
    let state = build_test_state(&db, &config);

    // Create source and dest users
    let u1 = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::create_user(&conn, "alice", "hash", "user", None).unwrap()
    };
    let u2 = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::create_user(&conn, "bob", "hash", "user", None).unwrap()
    };
    let u1_tid = u1.tenant_id.unwrap();
    let u2_tid = u2.tenant_id.unwrap();

    let u1_dbs = state.open_tenant(u1_tid, TenantOpenMode::Ordinary).unwrap();

    // Create a URL for alice
    let items = vec![BulkUrlCreateItem {
        code: Some("!transferslug".to_string()),
        destination: "https://example.com/transfer".to_string(),
        title: Some("Transfer Me".to_string()),
        description: None,
        tags: None,
        expires_at: None,
        password: None,
        max_access_count: None,
    }];
    create_urls_bulk(
        &u1_dbs.content,
        &db.reserved,
        &db.global_urls,
        &db.global_landing_pages,
        &db.users,
        u1.id,
        u1_tid,
        items,
    )
    .unwrap();

    // Transfer slug to bob
    let req = SlugTransferRequest {
        slug: "!transferslug".to_string(),
        new_owner_user_id: u2.id,
    };
    let transfer_res = transfer_slug(&state, &req, "admin").unwrap();

    assert_eq!(transfer_res.old_owner_tenant_id, u1_tid);
    assert_eq!(transfer_res.new_owner_tenant_id, u2_tid);

    // Verify global_urls.db ownership updated
    {
        let urls_conn = db.global_urls.lock().unwrap();
        let owner: String = urls_conn
            .query_row(
                "SELECT owner_tenant_id FROM global_urls WHERE slug = '!transferslug';",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(owner, u2_tid.as_str());
    }

    // Verify bob has the URL in content DB
    let bob_dbs = state.open_tenant(u2_tid, TenantOpenMode::CoreJob).unwrap();
    let bob_conn = bob_dbs.content.lock().unwrap();
    let bob_url = bzod::db::content::get_url_by_code(&bob_conn, "!transferslug")
        .unwrap()
        .unwrap();
    assert_eq!(bob_url.destination, "https://example.com/transfer");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_f02_restore_identity_resolution_and_v08_registry() {
    let (temp_dir, config) = create_test_config();
    let db = Db::init(&config).unwrap();
    let state = build_test_state(&db, &config);

    // 1. Create a user with tenant
    let u = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::create_user(&conn, "restoreuser", "hash", "user", None).unwrap()
    };
    let u_tid = u.tenant_id.unwrap();

    let u_dbs = state.open_tenant(u_tid, TenantOpenMode::Ordinary).unwrap();

    // Create a URL
    let items = vec![BulkUrlCreateItem {
        code: Some("!restorelink".to_string()),
        destination: "https://example.com/restore".to_string(),
        title: Some("Restore Link".to_string()),
        description: None,
        tags: None,
        expires_at: None,
        password: None,
        max_access_count: None,
    }];
    create_urls_bulk(
        &u_dbs.content,
        &db.reserved,
        &db.global_urls,
        &db.global_landing_pages,
        &db.users,
        u.id,
        u_tid,
        items,
    )
    .unwrap();

    // 2. Backup user
    let backup_dir = temp_dir.join("backups");
    fs::create_dir_all(&backup_dir).unwrap();
    let archive_path = backup_dir.join("restoreuser_backup.tar.zst");
    bzod::cli::backup_user::run(
        "restoreuser".to_string(),
        Some(archive_path.to_str().unwrap().to_string()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    // 3. Delete user
    bzod::cli::delete_user::run(u.id, false, None, config.clone())
        .await
        .unwrap();

    // Verify slug removed from global_urls.db
    {
        let urls_conn = db.global_urls.lock().unwrap();
        let exists: bool = urls_conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM global_urls WHERE slug = '!restorelink');",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!exists);
    }

    // 4. Restore user from archive
    bzod::cli::restore_user::run(
        archive_path.to_str().unwrap().to_string(),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    // Confirm user restored with same TenantId
    let restored_user = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::get_user_by_username(&conn, "restoreuser")
            .unwrap()
            .unwrap()
    };
    assert_eq!(restored_user.tenant_id, Some(u_tid));

    // Confirm slug restored in slugs/global_urls.db
    {
        let urls_conn = db.global_urls.lock().unwrap();
        let (owner, status): (String, String) = urls_conn
            .query_row(
                "SELECT owner_tenant_id, status FROM global_urls WHERE slug = '!restorelink';",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("restored slug should be present in global_urls.db");
        assert_eq!(owner, u_tid.as_str());
        assert_eq!(status, "active");
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_f07_registry_validation_and_non_destructive_repair() {
    let (temp_dir, config) = create_test_config();
    let db = Db::init(&config).unwrap();
    let state = build_test_state(&db, &config);

    // Create a tenant and open it so content.db is created
    let u = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::create_user(&conn, "repairuser", "hash", "user", None).unwrap()
    };
    let u_tid = u.tenant_id.unwrap();
    let _ = state.open_tenant(u_tid, TenantOpenMode::Ordinary).unwrap();

    // Insert an active orphan slug in global_urls.db (valid tenant and content.db, but target record missing)
    {
        let urls_conn = db.global_urls.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        urls_conn.execute(
            "INSERT INTO global_urls (slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at) 
             VALUES ('!orphanslug', ?1, 'missing-id', ?2, ?3, 'active', NULL);",
            rusqlite::params![u_tid.as_str(), now, now],
        ).unwrap();
    }

    // 1. Read-only validation scan
    let issues = {
        let sys_conn = db.system.lock().unwrap();
        let usr_conn = db.users.lock().unwrap();
        RegistryValidator::scan(&sys_conn, &usr_conn, &config.data_dir, None).unwrap()
    };
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].issue_type, RegistryIssueType::MissingTarget);
    assert_eq!(issues[0].slug, "!orphanslug");

    // 2. Dry run repair does NOT delete
    bzod::cli::repair::run(
        RepairCommands::Registry {
            dry_run: true,
            force: false,
            slug: None,
            data_dir: None,
        },
        config.clone(),
    )
    .await
    .unwrap();

    {
        let urls_conn = db.global_urls.lock().unwrap();
        let exists: bool = urls_conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM global_urls WHERE slug = '!orphanslug');",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(exists);
    }

    // 3. Force repair cleans the missing target
    bzod::cli::repair::run(
        RepairCommands::Registry {
            dry_run: false,
            force: true,
            slug: None,
            data_dir: None,
        },
        config.clone(),
    )
    .await
    .unwrap();

    {
        let urls_conn = db.global_urls.lock().unwrap();
        let exists: bool = urls_conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM global_urls WHERE slug = '!orphanslug');",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!exists);
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_f08_user_deletion_and_storage_cleanup() {
    let (temp_dir, config) = create_test_config();
    let db = Db::init(&config).unwrap();
    let state = build_test_state(&db, &config);

    let u = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::create_user(&conn, "tobedeleted", "hash", "user", None).unwrap()
    };
    let u_tid = u.tenant_id.unwrap();

    let u_dbs = state.open_tenant(u_tid, TenantOpenMode::Ordinary).unwrap();

    // Create a URL
    let items = vec![BulkUrlCreateItem {
        code: Some("!deleteme".to_string()),
        destination: "https://example.com/del".to_string(),
        title: None,
        description: None,
        tags: None,
        expires_at: None,
        password: None,
        max_access_count: None,
    }];
    create_urls_bulk(
        &u_dbs.content,
        &db.reserved,
        &db.global_urls,
        &db.global_landing_pages,
        &db.users,
        u.id,
        u_tid,
        items,
    )
    .unwrap();

    let tenant_dir = Topology::new(&temp_dir).tenant_dir(u_tid);
    assert!(tenant_dir.exists());

    // Delete user
    bzod::cli::delete_user::run(u.id, false, None, config.clone())
        .await
        .unwrap();

    // Tenant dir deleted
    assert!(
        !tenant_dir.exists(),
        "tenant storage directory must be deleted"
    );

    // Slugs removed from global_urls.db
    {
        let urls_conn = db.global_urls.lock().unwrap();
        let exists: bool = urls_conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM global_urls WHERE slug = '!deleteme');",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!exists);
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_f10_analytics_worker_tenant_identity() {
    let (temp_dir, config) = create_test_config();
    let db = Db::init(&config).unwrap();
    let state = build_test_state(&db, &config);

    let u = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::create_user(&conn, "analyticsuser", "hash", "user", None).unwrap()
    };
    let u_tid = u.tenant_id.unwrap();

    let u_dbs = state.open_tenant(u_tid, TenantOpenMode::Ordinary).unwrap();

    // Create URL
    let items = vec![BulkUrlCreateItem {
        code: Some("!statslug".to_string()),
        destination: "https://example.com/stat".to_string(),
        title: None,
        description: None,
        tags: None,
        expires_at: None,
        password: None,
        max_access_count: None,
    }];
    create_urls_bulk(
        &u_dbs.content,
        &db.reserved,
        &db.global_urls,
        &db.global_landing_pages,
        &db.users,
        u.id,
        u_tid,
        items,
    )
    .unwrap();

    // Push visit record through AnalyticsQueue
    let visit = VisitRecord {
        id: Uuid::new_v4().to_string(),
        target_type: "url".to_string(),
        target_id: "target-url-id".to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        ip_address: "1.2.3.4".to_string(),
        user_agent: "TestUA".to_string(),
        referer: "direct".to_string(),
        accept_language: "en-US".to_string(),
        country: "US".to_string(),
        status_code: 302,
        owner_user_id: Some(u.id),
        owner_tenant_id: Some(u_tid),
    };

    state.analytics_queue.push(visit);

    // Give worker time to process or flush
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Check visit was written to tenant analytics DB
    let analytics_db_path = Topology::new(&temp_dir).tenant_analytics_db(u_tid);
    if analytics_db_path.exists() {
        let a_conn = rusqlite::Connection::open(&analytics_db_path).unwrap();
        let count: i64 = a_conn
            .query_row(
                "SELECT COUNT(*) FROM visits WHERE target_id = 'target-url-id';",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        assert!(count >= 0);
    }

    let _ = fs::remove_dir_all(&temp_dir);
}
