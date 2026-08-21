use bzod::config::Config;
use bzod::db::Db;
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
async fn test_soft_delete_reserves_slug() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_soft_del_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");
    let urls_conn = db.global_urls.lock().unwrap();
    let reserved_conn = db.reserved.lock().unwrap();
    let pages_conn = db.global_landing_pages.lock().unwrap();

    // Register slug
    let now = chrono::Utc::now().to_rfc3339();
    urls_conn
        .execute(
            "INSERT INTO global_urls (slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at)
             VALUES ('!slug-to-delete', 't-tenant10', 'url_123', ?1, ?2, 'active', NULL);",
            rusqlite::params![now, now],
        )
        .unwrap();

    // Soft delete slug (disable it)
    urls_conn
        .execute(
            "UPDATE global_urls SET status = 'disabled', retired_at = ?1 WHERE slug = ?2;",
            rusqlite::params![now, "!slug-to-delete"],
        )
        .unwrap();

    // Verify it is not available (keeps the slug reserved)
    let avail = bzod::db::slugs::is_slug_available(
        &reserved_conn,
        &urls_conn,
        &pages_conn,
        "!slug-to-delete",
    )
    .unwrap();
    assert!(!avail);

    let status: String = urls_conn
        .query_row(
            "SELECT status FROM global_urls WHERE slug = ?1;",
            ["!slug-to-delete"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "disabled");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_permanent_delete_releases_slug() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_perm_del_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");
    let urls_conn = db.global_urls.lock().unwrap();
    let reserved_conn = db.reserved.lock().unwrap();
    let pages_conn = db.global_landing_pages.lock().unwrap();

    // Register slug
    let now = chrono::Utc::now().to_rfc3339();
    urls_conn
        .execute(
            "INSERT INTO global_urls (slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at)
             VALUES ('!slug-to-purge', 't-tenant10', 'url_456', ?1, ?2, 'active', NULL);",
            rusqlite::params![now, now],
        )
        .unwrap();

    // Release global slug (permanent deletion)
    urls_conn
        .execute(
            "DELETE FROM global_urls WHERE slug = ?1;",
            ["!slug-to-purge"],
        )
        .unwrap();

    // Verify slug is released (available for reuse)
    let avail = bzod::db::slugs::is_slug_available(
        &reserved_conn,
        &urls_conn,
        &pages_conn,
        "!slug-to-purge",
    )
    .unwrap();
    assert!(avail);

    let _ = fs::remove_dir_all(&temp_dir);
}
