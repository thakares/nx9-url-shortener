use bzod::config::Config;
use bzod::db::Db;
use chrono::Utc;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

fn create_temp_config(temp_dir: PathBuf) -> Config {
    let mut config = Config::load();
    config.data_dir = temp_dir.clone();
    config.backup_dir = temp_dir.clone();
    config.base_url = Some("http://bzo.in".to_string());
    config
}

#[tokio::test]
async fn test_content_flagging_and_disabling() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_mod_flag_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");

    let urls_conn = db.global_urls.lock().unwrap();

    // Register slug
    let now = Utc::now().to_rfc3339();
    urls_conn
        .execute(
            "INSERT INTO global_urls (slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at)
             VALUES ('!badslug', 't-tenant10', 'url_abc', ?1, ?2, 'active', NULL);",
            rusqlite::params![now, now],
        )
        .unwrap();

    // Verify it is active initially
    let status: String = urls_conn
        .query_row(
            "SELECT status FROM global_urls WHERE slug = ?1;",
            ["!badslug"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "active");

    // Moderate/disable slug
    urls_conn
        .execute(
            "UPDATE global_urls SET status = 'disabled' WHERE slug = ?1;",
            ["!badslug"],
        )
        .unwrap();

    // Verify it is disabled
    let status2: String = urls_conn
        .query_row(
            "SELECT status FROM global_urls WHERE slug = ?1;",
            ["!badslug"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status2, "disabled");

    // Moderate/re-enable slug
    urls_conn
        .execute(
            "UPDATE global_urls SET status = 'active' WHERE slug = ?1;",
            ["!badslug"],
        )
        .unwrap();

    let status3: String = urls_conn
        .query_row(
            "SELECT status FROM global_urls WHERE slug = ?1;",
            ["!badslug"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status3, "active");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_moderation_event_logging() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_mod_logging_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");
    let system_conn = db.system.lock().unwrap();

    let event_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    system_conn.execute(
        "INSERT INTO moderation_events (id, timestamp, admin_username, target_user_id, target_username, resource_type, resource_identifier, action, severity, reason)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10);",
        rusqlite::params![event_id, now, "admin", 10, "testuser", "slug", "!badslug", "block", "high", "abusive content"],
    ).unwrap();

    // Verify logged
    let count: i64 = system_conn.query_row(
        "SELECT COUNT(*) FROM moderation_events WHERE target_user_id = 10 AND resource_identifier = '!badslug';",
        [],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(count, 1);

    let _ = fs::remove_dir_all(&temp_dir);
}
