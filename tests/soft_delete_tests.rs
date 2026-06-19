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
    let system_conn = db.system.lock().unwrap();

    // Register slug
    bzod::db::users::register_global_slug(&system_conn, "!slug-to-delete", 10, "url", "url_123")
        .unwrap();

    // Soft delete slug
    bzod::db::users::soft_delete_global_slug(&system_conn, "!slug-to-delete", 10).unwrap();

    // Verify it is not available (keeps the slug reserved)
    let avail = bzod::db::users::is_slug_available(&system_conn, "!slug-to-delete").unwrap();
    assert!(!avail);

    let status: String = system_conn
        .query_row(
            "SELECT status FROM global_slugs WHERE slug = ?1;",
            ["!slug-to-delete"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "soft_deleted");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_permanent_delete_releases_slug() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_perm_del_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");
    let system_conn = db.system.lock().unwrap();

    // Register slug
    bzod::db::users::register_global_slug(&system_conn, "!slug-to-purge", 10, "url", "url_456")
        .unwrap();

    // Release global slug (permanent deletion)
    bzod::db::users::release_global_slug(&system_conn, "!slug-to-purge", 10).unwrap();

    // Verify slug is released (available for reuse)
    let avail = bzod::db::users::is_slug_available(&system_conn, "!slug-to-purge").unwrap();
    assert!(avail);

    let _ = fs::remove_dir_all(&temp_dir);
}
