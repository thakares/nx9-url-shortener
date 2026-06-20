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
async fn test_global_slug_uniqueness() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_slug_uniq_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");

    let system_conn = db.system.lock().unwrap();

    // Register a slug
    bzod::db::users::register_global_slug(&system_conn, "!myslug", 1, "url", "url1", "active")
        .unwrap();

    // Verify it is not available
    let avail = bzod::db::users::is_slug_available(&system_conn, "!myslug").unwrap();
    assert!(!avail);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_reserved_slug_rejection() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_reserved_slug_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");
    let system_conn = db.system.lock().unwrap();

    let reserved = vec![
        "admin",
        "login",
        "logout",
        "dashboard",
        "api",
        "docs",
        "assets",
        "static",
        "favicon.ico",
        "robots.txt",
        "health",
        "metrics",
        "install",
        "setup",
        "support",
        "help",
        "security",
        "abuse",
        "billing",
        "status",
        "legacy_admin",
        "administrator",
        "system",
        "root",
        "www",
    ];

    for slug in reserved {
        let avail = bzod::db::users::is_slug_available(&system_conn, slug).unwrap();
        assert!(!avail, "Reserved slug '{}' should not be available", slug);
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_slug_release_on_user_deletion() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_slug_release_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");

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

    // Register slug for this user
    {
        let system_conn = db.system.lock().unwrap();
        bzod::db::users::register_global_slug(
            &system_conn,
            "!user-slug",
            user_id,
            "url",
            "url_xyz",
            "active",
        )
        .unwrap();
        let avail = bzod::db::users::is_slug_available(&system_conn, "!user-slug").unwrap();
        assert!(!avail);
    }

    // Delete user
    bzod::cli::delete_user::run(user_id, false, None, config.clone())
        .await
        .unwrap();

    // Slug should now be released and available
    {
        let system_conn = db.system.lock().unwrap();
        let avail = bzod::db::users::is_slug_available(&system_conn, "!user-slug").unwrap();
        assert!(avail);

        // Verify history populated
        let count: i64 = system_conn
            .query_row(
                "SELECT COUNT(*) FROM slug_history WHERE slug = ?1 AND action = 'deleted';",
                ["!user-slug"],
                |row| row.get(0),
            )
            .unwrap();
        assert!(count > 0);
    }

    let _ = fs::remove_dir_all(&temp_dir);
}
