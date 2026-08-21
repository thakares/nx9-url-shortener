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
async fn test_quota_limit_enforcement() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_quota_limits_{}", uuid::Uuid::new_v4()));
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

    // Set max_urls to 1
    {
        let conn = db.users.lock().unwrap();
        conn.execute(
            "UPDATE quotas SET max_urls = 1 WHERE user_id = ?1;",
            [user_id],
        )
        .unwrap();
    }

    // Attempt to add URLs and manually increment count
    {
        let users_conn = db.users.lock().unwrap();
        let quota = bzod::db::users::get_user_quotas(&users_conn, user_id)
            .unwrap()
            .unwrap();
        assert_eq!(quota.max_urls, 1);
        assert_eq!(quota.current_urls, 0);

        // 1st Url: Success
        bzod::db::users::increment_quota_counter(&users_conn, user_id, "urls").unwrap();
        let quota1 = bzod::db::users::get_user_quotas(&users_conn, user_id)
            .unwrap()
            .unwrap();
        assert_eq!(quota1.current_urls, 1);

        // 2nd Url: Quota check fails
        let quota_check = quota1.current_urls >= quota1.max_urls;
        assert!(quota_check);
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_quota_reconcile_job() {
    let temp_dir = std::env::temp_dir().join(format!(
        "bzod_test_quota_reconcile_{}",
        uuid::Uuid::new_v4()
    ));
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

    // Introduce drift manually: make current_urls = 10, when actually 1 URL exists
    {
        let user_content_conn = bzod::jobs::open_user_content_conn(&db, user_id).unwrap();
        bzod::db::content::create_url_extended(
            &user_content_conn,
            "!my-reconcile-slug",
            "https://example.com",
            None,
            None,
            &vec![],
            None,
            None,
            None,
        )
        .unwrap();

        let users_conn = db.users.lock().unwrap();
        users_conn
            .execute(
                "UPDATE quotas SET current_urls = 10 WHERE user_id = ?1;",
                [user_id],
            )
            .unwrap();
    }

    // Run reconciliation
    {
        let user_content_conn = bzod::jobs::open_user_content_conn(&db, user_id).unwrap();
        let users_conn = db.users.lock().unwrap();
        bzod::db::users::reconcile_user_quotas(&users_conn, user_id, &user_content_conn).unwrap();

        // Verify drift is repaired
        let quota = bzod::db::users::get_user_quotas(&users_conn, user_id)
            .unwrap()
            .unwrap();
        assert_eq!(quota.current_urls, 1);
    }

    let _ = fs::remove_dir_all(&temp_dir);
}
