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
async fn test_user_creation() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_creation_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");

    let res = bzod::cli::create_user::run(
        Some("testuser".to_string()),
        Some("password123".to_string()),
        None,
        config.clone(),
    )
    .await;
    assert!(res.is_ok());

    // Verify user directories and files
    let conn = db.users.lock().unwrap();
    let user = bzod::db::users::get_user_by_username(&conn, "testuser")
        .unwrap()
        .unwrap();
    assert_eq!(user.status, "active");

    let user_dir = bzod::db::tenant::location_for_user(&user)
        .unwrap()
        .dir(&bzod::db::topology::Topology::new(&temp_dir))
        .unwrap();
    assert!(user_dir.join("content.db").exists());
    assert!(user_dir.join("analytics.db").exists());
    assert!(user_dir.join("profile.db").exists());

    // Verify quotas are seeded
    let quota = bzod::db::users::get_user_quotas(&conn, user.id)
        .unwrap()
        .unwrap();
    assert_eq!(quota.max_urls, 100);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_user_disable_enable() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_disable_enable_{}", uuid::Uuid::new_v4()));
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

    // Disable the user
    bzod::cli::disable_user::run(user_id, None, config.clone())
        .await
        .unwrap();

    {
        let conn = db.users.lock().unwrap();
        let user = bzod::db::users::get_user_by_id(&conn, user_id)
            .unwrap()
            .unwrap();
        assert_eq!(user.status, "disabled");

        // Verify authentication fails for disabled users
        let api_key = "Bearer test_api_token";
        let auth_res = bzod::auth::authenticate_api_key(&db.admin.lock().unwrap(), &conn, api_key);
        assert!(auth_res.unwrap().is_none());
    }

    // Re-enable the user
    bzod::cli::enable_user::run(user_id, None, config.clone())
        .await
        .unwrap();

    {
        let conn = db.users.lock().unwrap();
        let user = bzod::db::users::get_user_by_id(&conn, user_id)
            .unwrap()
            .unwrap();
        assert_eq!(user.status, "active");
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_password_reset() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_pwd_reset_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");

    let _ = bzod::cli::create_user::run(
        Some("testuser".to_string()),
        Some("oldpassword".to_string()),
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

    // Reset password
    bzod::cli::reset_password::run(
        user_id,
        Some("newpassword".to_string()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    {
        let conn = db.users.lock().unwrap();
        let user = bzod::db::users::get_user_by_id(&conn, user_id)
            .unwrap()
            .unwrap();

        // Verify password hash changed and verifies correctly
        assert!(bzod::auth::verify_password(
            "newpassword",
            &user.password_hash
        ));
        assert!(!bzod::auth::verify_password(
            "oldpassword",
            &user.password_hash
        ));
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_user_status_transitions() {
    let temp_dir = std::env::temp_dir().join(format!(
        "bzod_test_status_transitions_{}",
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

    let conn = db.users.lock().unwrap();

    // active -> disabled
    bzod::db::users::update_user_status(&conn, user_id, "disabled").unwrap();
    let u1 = bzod::db::users::get_user_by_id(&conn, user_id)
        .unwrap()
        .unwrap();
    assert_eq!(u1.status, "disabled");

    // disabled -> active
    bzod::db::users::update_user_status(&conn, user_id, "active").unwrap();
    let u2 = bzod::db::users::get_user_by_id(&conn, user_id)
        .unwrap()
        .unwrap();
    assert_eq!(u2.status, "active");

    let _ = fs::remove_dir_all(&temp_dir);
}
