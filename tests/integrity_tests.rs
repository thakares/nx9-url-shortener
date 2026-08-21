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
async fn test_global_slug_index_consistency() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_integrity_{}", uuid::Uuid::new_v4()));
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

    let (user_id, tid) = {
        let conn = db.users.lock().unwrap();
        let u = bzod::db::users::get_user_by_username(&conn, "testuser")
            .unwrap()
            .unwrap();
        (u.id, u.tenant_id.unwrap())
    };

    // Add a URL for the user
    let url_id = {
        let conn = bzod::jobs::open_user_content_conn(&db, user_id).unwrap();
        let url = bzod::db::content::create_url_extended(
            &conn,
            "!integ-slug",
            "https://google.com",
            None,
            None,
            &vec![],
            None,
            None,
            None,
        )
        .unwrap();
        url.id
    };

    // Register globally
    {
        let urls_conn = db.global_urls.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        urls_conn
            .execute(
                "INSERT INTO global_urls (slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at)
                 VALUES ('!integ-slug', ?1, ?2, ?3, ?4, 'active', NULL);",
                rusqlite::params![tid.as_str(), &url_id, now, now],
            )
            .unwrap();
    }

    // Consistency Check:
    // 1. Every active slug in user databases exists in global_urls
    {
        let users = {
            let conn = db.users.lock().unwrap();
            bzod::db::users::list_users(&conn).unwrap()
        };

        let urls_conn = db.global_urls.lock().unwrap();
        for u in users {
            if let Some(tenant_id) = u.tenant_id {
                let conn = bzod::jobs::open_user_content_conn(&db, u.id).unwrap();
                let mut stmt = conn.prepare("SELECT code FROM urls;").unwrap();
                let rows = stmt.query_map([], |row| row.get::<_, String>(0)).unwrap();

                for code_res in rows {
                    let code = code_res.unwrap();
                    let exists: bool = urls_conn
                        .query_row(
                            "SELECT EXISTS(SELECT 1 FROM global_urls WHERE slug = ?1 AND owner_tenant_id = ?2);",
                            rusqlite::params![code, tenant_id.as_str()],
                            |row| row.get(0),
                        )
                        .unwrap();
                    assert!(
                        exists,
                        "Active user slug '{}' missing from global_urls",
                        code
                    );
                }
            }
        }
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_slug_history_consistency() {
    let temp_dir = std::env::temp_dir().join(format!(
        "bzod_test_history_consistency_{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");
    let system_conn = db.system.lock().unwrap();

    // Verify slug history table columns exist and accept transition entries
    system_conn.execute(
        "INSERT INTO slug_history (slug, old_owner_user_id, new_owner_user_id, action, timestamp)
         VALUES ('!slug1', 1, 2, 'transferred', 'now');",
        [],
    ).unwrap();

    let count: i64 = system_conn
        .query_row(
            "SELECT COUNT(*) FROM slug_history WHERE slug = '!slug1' AND action = 'transferred';",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);

    let _ = fs::remove_dir_all(&temp_dir);
}
