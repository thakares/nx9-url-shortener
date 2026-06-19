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
async fn test_legacy_migration() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_migration_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    // Create legacy files at the root of temp_dir
    let legacy_admin_path = temp_dir.join("admin.db");
    let legacy_system_path = temp_dir.join("system.db");
    let legacy_content_path = temp_dir.join("content.db");
    let legacy_analytics_path = temp_dir.join("analytics.db");

    // Apply old schemas/migrations
    {
        let mut admin_conn = rusqlite::Connection::open(&legacy_admin_path).unwrap();
        bzod::db::migrations::run_migrations(
            &mut admin_conn,
            "admin",
            bzod::db::migrations::ADMIN_MIGRATIONS,
            None,
        )
        .unwrap();
        // Insert admin user
        let hash = bzod::auth::hash_password("admin_pass").unwrap();
        bzod::db::admin::create_user(&admin_conn, "admin", &hash).unwrap();

        let mut system_conn = rusqlite::Connection::open(&legacy_system_path).unwrap();
        bzod::db::migrations::run_migrations(
            &mut system_conn,
            "system",
            bzod::db::migrations::SYSTEM_MIGRATIONS,
            None,
        )
        .unwrap();

        let mut content_conn = rusqlite::Connection::open(&legacy_content_path).unwrap();
        bzod::db::migrations::run_migrations(
            &mut content_conn,
            "content",
            bzod::db::migrations::CONTENT_MIGRATIONS,
            None,
        )
        .unwrap();
        // Insert a link in legacy content
        bzod::db::content::create_url_extended(
            &content_conn,
            "!legacy-slug",
            "https://legacy.com",
            None,
            None,
            &[],
            None,
            None,
            None,
        )
        .unwrap();

        let mut analytics_conn = rusqlite::Connection::open(&legacy_analytics_path).unwrap();
        bzod::db::migrations::run_migrations(
            &mut analytics_conn,
            "analytics",
            bzod::db::migrations::ANALYTICS_MIGRATIONS,
            None,
        )
        .unwrap();
    }

    // Call Db::init which triggers the legacy migration
    let db = Db::init(&config).expect("Failed to init Db and run legacy migration");

    // Verify files moved to correct directories
    assert!(!legacy_admin_path.exists());
    assert!(!legacy_content_path.exists());
    assert!(temp_dir.join("admin/admin.db").exists());
    assert!(temp_dir.join("users/1/content.db").exists());

    // Verify legacy_admin created as system and disabled
    {
        let conn = db.users.lock().unwrap();
        let user = bzod::db::users::get_user_by_id(&conn, 1).unwrap().unwrap();
        assert_eq!(user.username, "legacy_admin");
        assert_eq!(user.status, "disabled");
        assert_eq!(user.account_type, "system");
    }

    // Verify slug registered in global_slugs
    {
        let conn = db.system.lock().unwrap();
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM global_slugs WHERE slug = '!legacy-slug');",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(exists);
    }

    let _ = fs::remove_dir_all(&temp_dir);
}
