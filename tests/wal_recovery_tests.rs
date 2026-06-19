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
async fn test_wal_recovery_after_backup() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_test_wal_rec_{}", uuid::Uuid::new_v4()));
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

    // 1. Write some content
    {
        let conn = bzod::jobs::open_user_content_conn(&db, user_id).unwrap();
        bzod::db::content::create_url_extended(
            &conn,
            "!wal-slug",
            "https://google.com/wal",
            None,
            None,
            &vec![],
            None,
            None,
            None,
        )
        .unwrap();
    }

    // 2. Perform backup (this checkpoints and flushes WAL to DB files)
    let backup_file = temp_dir.join("testuser-backup.tar.zst");
    bzod::cli::backup_user::run(
        "testuser".to_string(),
        Some(backup_file.to_string_lossy().to_string()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    // 3. Restore user
    bzod::cli::restore_user::run(
        backup_file.to_string_lossy().to_string(),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    // 4. Verify that restored databases are consistent and can successfully perform WAL writes
    {
        let conn = bzod::jobs::open_user_content_conn(&db, user_id).unwrap();

        // SQLite integrity check
        bzod::db::sqlite::integrity_check(&conn, "content").unwrap();

        // Write a new url after restore to verify WAL write works
        bzod::db::content::create_url_extended(
            &conn,
            "!new-wal-slug",
            "https://google.com/new-wal",
            None,
            None,
            &vec![],
            None,
            None,
            None,
        )
        .unwrap();

        let url = bzod::db::content::get_url_by_code(&conn, "!new-wal-slug")
            .unwrap()
            .unwrap();
        assert_eq!(url.destination, "https://google.com/new-wal");
    }

    let _ = fs::remove_dir_all(&temp_dir);
}
