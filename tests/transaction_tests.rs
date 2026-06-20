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

    let system_conn = db.system.lock().unwrap();

    // Insert a reserving slug older than 15 minutes (e.g. 20 minutes ago)
    let old_time = (chrono::Utc::now() - chrono::Duration::minutes(20)).to_rfc3339();
    system_conn.execute(
        "INSERT INTO global_slugs (slug, owner_user_id, target_type, target_id, created_at, updated_at, status)
         VALUES ('stale-slug', 2, 'url', '', ?1, ?1, 'reserving')",
        [&old_time]
    ).unwrap();

    // Verify it exists before cleanup
    let exists: bool = system_conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM global_slugs WHERE slug = 'stale-slug')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(exists);

    // Run cleanup
    let cleaned = bzod::db::users::cleanup_stale_reservations(&system_conn, &temp_dir).unwrap();
    assert_eq!(cleaned, 1);

    // Verify it is gone
    let exists: bool = system_conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM global_slugs WHERE slug = 'stale-slug')",
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

    let system_conn = db.system.lock().unwrap();

    // Insert a reserving slug that is brand new (e.g. 1 minute ago)
    let new_time = (chrono::Utc::now() - chrono::Duration::minutes(1)).to_rfc3339();
    system_conn.execute(
        "INSERT INTO global_slugs (slug, owner_user_id, target_type, target_id, created_at, updated_at, status)
         VALUES ('fresh-slug', 2, 'url', '', ?1, ?1, 'reserving')",
        [&new_time]
    ).unwrap();

    // Run cleanup
    let cleaned = bzod::db::users::cleanup_stale_reservations(&system_conn, &temp_dir).unwrap();
    assert_eq!(cleaned, 0);

    // Verify it is still there
    let exists: bool = system_conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM global_slugs WHERE slug = 'fresh-slug')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(exists);

    let _ = fs::remove_dir_all(&temp_dir);
}
