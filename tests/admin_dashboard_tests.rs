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
async fn test_admin_user_listing() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_admin_list_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");

    let _ = bzod::cli::create_user::run(
        Some("usera".to_string()),
        Some("password123".to_string()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    let conn = db.users.lock().unwrap();
    let users = bzod::db::users::list_users(&conn).unwrap();

    // Verify list contains legacy_admin and usera
    assert!(users.len() >= 2);
    assert!(users.iter().any(|u| u.username == "legacy_admin"));
    assert!(users.iter().any(|u| u.username == "usera"));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_admin_user_statistics() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_admin_stats_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");

    let _ = bzod::cli::create_user::run(
        Some("usera".to_string()),
        Some("password123".to_string()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    let user_id = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::get_user_by_username(&conn, "usera")
            .unwrap()
            .unwrap()
            .id
    };

    // Aggregate statistics (e.g. check current quotas match the content databases)
    {
        let users_conn = db.users.lock().unwrap();
        let quota = bzod::db::users::get_user_quotas(&users_conn, user_id)
            .unwrap()
            .unwrap();

        // Assert initial counts
        assert_eq!(quota.current_urls, 0);
        assert_eq!(quota.current_landings, 0);
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_admin_system_settings() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_admin_settings_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");

    let system_conn = db.system.lock().unwrap();

    // Verify initial settings are loaded
    let recon_hours: String = system_conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'quota_reconcile_interval_hours';",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(recon_hours, "24");

    // Update settings
    system_conn
        .execute(
            "UPDATE settings SET value = '12' WHERE key = 'quota_reconcile_interval_hours';",
            [],
        )
        .unwrap();

    let recon_hours2: String = system_conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'quota_reconcile_interval_hours';",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(recon_hours2, "12");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_admin_moderation_listing() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_admin_mod_list_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");
    let system_conn = db.system.lock().unwrap();

    // Insert a moderation event
    system_conn.execute(
        "INSERT INTO moderation_events (id, timestamp, admin_username, target_user_id, target_username, resource_type, resource_identifier, action, severity, reason)
         VALUES ('evt1', 'now', 'admin', 10, 'user10', 'url', '!slug', 'block', 'high', 'violating content');",
        [],
    ).unwrap();

    // Query moderation events
    let mut stmt = system_conn
        .prepare("SELECT id, action, reason FROM moderation_events;")
        .unwrap();
    let events = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .unwrap();

    let event_list: Vec<_> = events.map(|r| r.unwrap()).collect();
    assert_eq!(event_list.len(), 1);
    assert_eq!(event_list[0].0, "evt1");
    assert_eq!(event_list[0].1, "block");

    let _ = fs::remove_dir_all(&temp_dir);
}
