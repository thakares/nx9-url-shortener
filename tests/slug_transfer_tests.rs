use bzod::config::Config;
use bzod::db::Db;
use chrono::Utc;
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
async fn test_slug_transfer() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_transfer_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");

    // Create User A
    let _ = bzod::cli::create_user::run(
        Some("usera".to_string()),
        Some("password123".to_string()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    // Create User B
    let _ = bzod::cli::create_user::run(
        Some("userb".to_string()),
        Some("password123".to_string()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    let (id_a, id_b) = {
        let conn = db.users.lock().unwrap();
        let a = bzod::db::users::get_user_by_username(&conn, "usera")
            .unwrap()
            .unwrap()
            .id;
        let b = bzod::db::users::get_user_by_username(&conn, "userb")
            .unwrap()
            .unwrap()
            .id;
        (a, b)
    };

    // User A creates a URL
    let conn_a = bzod::jobs::open_user_content_conn(&db, id_a).unwrap();
    let url = bzod::db::content::create_url_extended(
        &conn_a,
        "!trans-slug",
        "https://google.com/transfer",
        None,
        None,
        &vec![],
        None,
        None,
        None,
    )
    .unwrap();

    // Register globally
    {
        let system_conn = db.system.lock().unwrap();
        bzod::db::users::register_global_slug(&system_conn, "!trans-slug", id_a, "url", &url.id)
            .unwrap();

        let users_conn = db.users.lock().unwrap();
        bzod::db::users::increment_quota_counter(&users_conn, id_a, "urls").unwrap();
    }

    // Perform transfer to User B
    {
        let old_conn = bzod::jobs::open_user_content_conn(&db, id_a).unwrap();
        let new_conn = bzod::jobs::open_user_content_conn(&db, id_b).unwrap();

        // 1. Copy URL to new owner content.db
        let url_to_copy = bzod::db::content::get_url_by_code(&old_conn, "!trans-slug")
            .unwrap()
            .unwrap();
        bzod::db::content::create_url_extended(
            &new_conn,
            &url_to_copy.code,
            &url_to_copy.destination,
            url_to_copy.title.as_deref(),
            url_to_copy.description.as_deref(),
            &url_to_copy.tags,
            url_to_copy.expires_at.as_deref(),
            url_to_copy.password_hash.as_deref(),
            url_to_copy.max_access_count,
        )
        .unwrap();

        // 2. Delete from old owner
        bzod::db::content::delete_url(&old_conn, &url_to_copy.id).unwrap();

        // 3. Update global slugs and quotas
        let system_conn = db.system.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        system_conn
            .execute(
                "UPDATE global_slugs SET owner_user_id = ?1, updated_at = ?2 WHERE slug = ?3;",
                rusqlite::params![id_b, now, "!trans-slug"],
            )
            .unwrap();

        let users_conn = db.users.lock().unwrap();
        bzod::db::users::decrement_quota_counter(&users_conn, id_a, "urls").unwrap();
        bzod::db::users::increment_quota_counter(&users_conn, id_b, "urls").unwrap();
    }

    // Verify User A has no link, User B has it
    {
        let conn_a = bzod::jobs::open_user_content_conn(&db, id_a).unwrap();
        let conn_b = bzod::jobs::open_user_content_conn(&db, id_b).unwrap();
        assert!(bzod::db::content::get_url_by_code(&conn_a, "!trans-slug")
            .unwrap()
            .is_none());
        assert!(bzod::db::content::get_url_by_code(&conn_b, "!trans-slug")
            .unwrap()
            .is_some());

        // Verify quotas adjusted
        let users_conn = db.users.lock().unwrap();
        let q_a = bzod::db::users::get_user_quotas(&users_conn, id_a)
            .unwrap()
            .unwrap();
        let q_b = bzod::db::users::get_user_quotas(&users_conn, id_b)
            .unwrap()
            .unwrap();
        assert_eq!(q_a.current_urls, 0);
        assert_eq!(q_b.current_urls, 1);
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_slug_transfer_history_logging() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_trans_history_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");

    let system_conn = db.system.lock().unwrap();
    let now = Utc::now().to_rfc3339();

    // Log a slug transfer history entry
    system_conn.execute(
        "INSERT INTO slug_history (slug, old_owner_user_id, new_owner_user_id, action, timestamp, admin_username)
         VALUES (?1, ?2, ?3, 'transferred', ?4, ?5);",
        rusqlite::params!["!trans-slug", 10, 20, now, "admin"],
    ).unwrap();

    // Verify it is logged
    let count: i64 = system_conn.query_row(
        "SELECT COUNT(*) FROM slug_history WHERE slug = '!trans-slug' AND old_owner_user_id = 10 AND new_owner_user_id = 20;",
        [],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(count, 1);

    let _ = fs::remove_dir_all(&temp_dir);
}
