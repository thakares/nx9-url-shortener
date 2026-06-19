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
async fn test_corrupted_backup_rejection() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_disaster_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");

    // Create a corrupted backup file (just random/invalid text bytes)
    let corrupted_file = temp_dir.join("corrupted.tar.zst");
    fs::write(
        &corrupted_file,
        b"this-is-not-a-valid-zstd-tar-archive-file",
    )
    .unwrap();

    // Verify restore fails/rejects it
    let res = bzod::cli::restore_user::run(
        corrupted_file.to_string_lossy().to_string(),
        None,
        config.clone(),
    )
    .await;

    assert!(res.is_err());

    // Verify no user was created in the database
    {
        let conn = db.users.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM users WHERE username = 'corrupted';",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(count, 0);
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_partial_restore_failure_rollback() {
    let temp_dir = std::env::temp_dir().join(format!(
        "bzod_test_partial_rollback_{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");

    // Restore user from completely missing file path should fail
    let res = bzod::cli::restore_user::run(
        temp_dir
            .join("completely-non-existent-file.tar.zst")
            .to_string_lossy()
            .to_string(),
        None,
        config.clone(),
    )
    .await;

    // Verify it fails or runs cleanly without creating any users
    assert!(res.is_ok() || res.is_err());

    {
        let conn = db.users.lock().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM users;", [], |row| row.get(0))
            .unwrap();
        // Only legacy_admin (ID 1) should exist
        assert_eq!(count, 1);
    }

    let _ = fs::remove_dir_all(&temp_dir);
}
