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
async fn test_user_deletion_cleanup() {
    let temp_dir = std::env::temp_dir().join(format!(
        "bzod_test_deletion_cleanup_{}",
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

    // Add session and API token
    {
        let conn = db.users.lock().unwrap();
        let expires_at = (chrono::Utc::now() + chrono::Duration::seconds(3600)).to_rfc3339();
        bzod::db::users::create_user_session(&conn, "sess_abc", user_id, &expires_at).unwrap();
        bzod::db::users::create_user_api_token(&conn, user_id, "token_abc").unwrap();
    }

    // Delete user
    bzod::cli::delete_user::run(user_id, false, None, config.clone())
        .await
        .unwrap();

    // Verify directory is deleted
    let user_dir = temp_dir.join("users").join(user_id.to_string());
    assert!(!user_dir.exists());

    // Verify sessions/tokens are cascadingly deleted
    {
        let conn = db.users.lock().unwrap();
        let sess_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE user_id = ?1;",
                [user_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(sess_count, 0);

        let token_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM api_tokens WHERE user_id = ?1;",
                [user_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(token_count, 0);
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_user_deletion_audit_snapshot() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_deletion_audit_{}", uuid::Uuid::new_v4()));
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

    // Delete user
    bzod::cli::delete_user::run(user_id, false, None, config.clone())
        .await
        .unwrap();

    // Verify audit events and history entries are NOT deleted (retained)
    {
        let system_conn = db.system.lock().unwrap();
        let audit_count: i64 = system_conn.query_row(
            "SELECT COUNT(*) FROM audit_events WHERE action = 'USER_DELETION' AND object_id = ?1;",
            [user_id.to_string()],
            |row| row.get(0),
        ).unwrap();
        assert!(audit_count > 0);
    }

    let _ = fs::remove_dir_all(&temp_dir);
}
