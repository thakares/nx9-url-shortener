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
async fn test_cleanup_stale_reservations() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_tx_test_1_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let db = Db::init(&config).expect("Failed to init Db");

    let urls_conn = db.global_urls.lock().unwrap();
    let pages_conn = db.global_landing_pages.lock().unwrap();

    // Insert a reserving slug older than 15 minutes (e.g. 20 minutes ago)
    let old_time = (chrono::Utc::now() - chrono::Duration::minutes(20)).to_rfc3339();
    urls_conn
        .execute(
            "INSERT INTO global_urls (slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at)
             VALUES ('!stale-slug', 't-tenant1234', '', ?1, ?1, 'reserving', NULL)",
            [&old_time],
        )
        .unwrap();

    // Verify it exists before cleanup
    let exists: bool = urls_conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM global_urls WHERE slug = '!stale-slug')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(exists);

    // Run cleanup for records older than 15 minutes (900 seconds)
    let cleaned =
        bzod::db::slugs::cleanup_stale_reservations(&urls_conn, &pages_conn, 900).unwrap();
    assert_eq!(cleaned, 1);

    // Verify it is gone
    let exists: bool = urls_conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM global_urls WHERE slug = '!stale-slug')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!exists);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_cleanup_preserves_valid_reservations() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_tx_test_2_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let db = Db::init(&config).expect("Failed to init Db");

    let urls_conn = db.global_urls.lock().unwrap();
    let pages_conn = db.global_landing_pages.lock().unwrap();

    // Insert a reserving slug that is brand new (e.g. 1 minute ago)
    let new_time = (chrono::Utc::now() - chrono::Duration::minutes(1)).to_rfc3339();
    urls_conn
        .execute(
            "INSERT INTO global_urls (slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at)
             VALUES ('!fresh-slug', 't-tenant1234', '', ?1, ?1, 'reserving', NULL)",
            [&new_time],
        )
        .unwrap();

    // Run cleanup for records older than 15 minutes (900 seconds)
    let cleaned =
        bzod::db::slugs::cleanup_stale_reservations(&urls_conn, &pages_conn, 900).unwrap();
    assert_eq!(cleaned, 0);

    // Verify it is still there
    let exists: bool = urls_conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM global_urls WHERE slug = '!fresh-slug')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(exists);

    let _ = fs::remove_dir_all(&temp_dir);
}
