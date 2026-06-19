use bzod::config::Config;
use bzod::db::Db;
use std::fs;
use std::fs::File;
use std::path::PathBuf;
use tar::Archive;
use zstd::Decoder;

fn create_temp_config(temp_dir: PathBuf) -> Config {
    let mut config = Config::load();
    config.data_dir = temp_dir.clone();
    config.backup_dir = temp_dir.clone();
    config.base_url = Some("http://bzo.in".to_string());
    config
}

#[derive(serde::Serialize, serde::Deserialize)]
struct UserBackupMetadata {
    id: i64,
    username: String,
    password_hash: String,
    status: String,
    created_at: String,
    account_type: String,
    metadata: Option<String>,
    quotas: UserBackupQuotas,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct UserBackupQuotas {
    max_urls: i64,
    max_landings: i64,
    max_api_tokens: i64,
    max_storage_mb: i64,
}

#[tokio::test]
async fn test_backup_metadata_integrity() {
    let temp_dir = std::env::temp_dir().join(format!(
        "bzod_test_backup_integrity_{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let _db = Db::init(&config).expect("Failed to init Db");

    let _ = bzod::cli::create_user::run(
        Some("testuser".to_string()),
        Some("password123".to_string()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    let backup_file = temp_dir.join("testuser-backup.tar.zst");
    bzod::cli::backup_user::run(
        "testuser".to_string(),
        Some(backup_file.to_string_lossy().to_string()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    // Verify archive contains metadata.json and it has valid fields
    let f = File::open(&backup_file).unwrap();
    let zst_dec = Decoder::new(f).unwrap();
    let mut archive = Archive::new(zst_dec);

    let mut found_metadata = false;
    for entry_res in archive.entries().unwrap() {
        let mut entry = entry_res.unwrap();
        let path = entry.path().unwrap();
        let file_name = path.file_name().unwrap().to_str().unwrap();
        if file_name == "metadata.json" {
            found_metadata = true;
            let meta: UserBackupMetadata = serde_json::from_reader(&mut entry).unwrap();
            assert_eq!(meta.username, "testuser");
            assert_eq!(meta.quotas.max_urls, 100);
            break;
        }
    }
    assert!(found_metadata);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_backup_restore_roundtrip() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_backup_rt_{}", uuid::Uuid::new_v4()));
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

    // Add a link for user
    {
        let user_content_conn = bzod::jobs::open_user_content_conn(&db, user_id).unwrap();
        bzod::db::content::create_url_extended(
            &user_content_conn,
            "!rt-slug",
            "https://example.com/rt",
            None,
            None,
            &vec![],
            None,
            None,
            None,
        )
        .unwrap();

        // Increment quota counter
        let users_conn = db.users.lock().unwrap();
        bzod::db::users::increment_quota_counter(&users_conn, user_id, "urls").unwrap();

        // Register global slug
        let system_conn = db.system.lock().unwrap();
        bzod::db::users::register_global_slug(&system_conn, "!rt-slug", user_id, "url", "rt-id")
            .unwrap();
    }

    // Backup user
    let backup_file = temp_dir.join("testuser-backup.tar.zst");
    bzod::cli::backup_user::run(
        "testuser".to_string(),
        Some(backup_file.to_string_lossy().to_string()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    // Delete user
    bzod::cli::delete_user::run(user_id, false, None, config.clone())
        .await
        .unwrap();

    // Restore user
    bzod::cli::restore_user::run(
        backup_file.to_string_lossy().to_string(),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    // Verify restored user
    {
        let conn = db.users.lock().unwrap();
        let user = bzod::db::users::get_user_by_username(&conn, "testuser")
            .unwrap()
            .unwrap();
        assert_eq!(user.status, "active");

        let quota = bzod::db::users::get_user_quotas(&conn, user.id)
            .unwrap()
            .unwrap();
        assert_eq!(quota.current_urls, 1);

        // Verify content restored
        let user_content_conn = bzod::jobs::open_user_content_conn(&db, user.id).unwrap();
        let url = bzod::db::content::get_url_by_code(&user_content_conn, "!rt-slug")
            .unwrap()
            .unwrap();
        assert_eq!(url.destination, "https://example.com/rt");
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_restore_slug_collision_rejection() {
    let temp_dir = std::env::temp_dir().join(format!(
        "bzod_test_restore_collision_{}",
        uuid::Uuid::new_v4()
    ));
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

    let id_a = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::get_user_by_username(&conn, "usera")
            .unwrap()
            .unwrap()
            .id
    };

    // Add a link for User A
    {
        let conn_a = bzod::jobs::open_user_content_conn(&db, id_a).unwrap();
        bzod::db::content::create_url_extended(
            &conn_a,
            "!collision-slug",
            "https://usera.com",
            None,
            None,
            &vec![],
            None,
            None,
            None,
        )
        .unwrap();

        let system_conn = db.system.lock().unwrap();
        bzod::db::users::register_global_slug(&system_conn, "!collision-slug", id_a, "url", "a-id")
            .unwrap();
    }

    // Backup User A
    let backup_file = temp_dir.join("usera-backup.tar.zst");
    bzod::cli::backup_user::run(
        "usera".to_string(),
        Some(backup_file.to_string_lossy().to_string()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    // Delete User A from the database so we can try restoring them
    bzod::cli::delete_user::run(id_a, false, None, config.clone())
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

    let id_b = {
        let conn = db.users.lock().unwrap();
        bzod::db::users::get_user_by_username(&conn, "userb")
            .unwrap()
            .unwrap()
            .id
    };

    // Add colliding slug for User B
    {
        let conn_b = bzod::jobs::open_user_content_conn(&db, id_b).unwrap();
        bzod::db::content::create_url_extended(
            &conn_b,
            "!collision-slug",
            "https://userb.com",
            None,
            None,
            &vec![],
            None,
            None,
            None,
        )
        .unwrap();

        let system_conn = db.system.lock().unwrap();
        bzod::db::users::register_global_slug(&system_conn, "!collision-slug", id_b, "url", "b-id")
            .unwrap();
    }

    // Attempt to restore User A from backup
    bzod::cli::restore_user::run(
        backup_file.to_string_lossy().to_string(),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    // Verify that User B still owns the slug in global_slugs and User A's slug registration was skipped/rejected
    {
        let system_conn = db.system.lock().unwrap();
        let owner_id: i64 = system_conn
            .query_row(
                "SELECT owner_user_id FROM global_slugs WHERE slug = '!collision-slug';",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(owner_id, id_b);
    }

    let _ = fs::remove_dir_all(&temp_dir);
}
