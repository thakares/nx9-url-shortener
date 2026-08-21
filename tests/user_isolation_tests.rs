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
async fn test_user_database_isolation() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_isolation_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");

    let _ = bzod::cli::create_user::run(
        Some("usera".to_string()),
        Some("password123".to_string()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    let _ = bzod::cli::create_user::run(
        Some("userb".to_string()),
        Some("password123".to_string()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    let (user_a, user_b) = {
        let conn = db.users.lock().unwrap();
        let a = bzod::db::users::get_user_by_username(&conn, "usera")
            .unwrap()
            .unwrap();
        let b = bzod::db::users::get_user_by_username(&conn, "userb")
            .unwrap()
            .unwrap();
        (a, b)
    };

    let topology = bzod::db::topology::Topology::new(&temp_dir);
    let dir_a = bzod::db::tenant::location_for_user(&user_a)
        .unwrap()
        .dir(&topology)
        .unwrap();
    let dir_b = bzod::db::tenant::location_for_user(&user_b)
        .unwrap()
        .dir(&topology)
        .unwrap();

    assert_ne!(dir_a, dir_b);
    assert!(dir_a.join("content.db").exists());
    assert!(dir_b.join("content.db").exists());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_cross_user_content_access_denied() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_cross_access_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");

    let _ = bzod::cli::create_user::run(
        Some("usera".to_string()),
        Some("password123".to_string()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

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

    // User A adds a URL
    let conn_a = bzod::jobs::open_user_content_conn(&db, id_a).unwrap();
    let _ = bzod::db::content::create_url_extended(
        &conn_a,
        "!slug-a",
        "https://google.com/usera",
        None,
        None,
        &vec!["tag_usera".to_string()],
        None,
        None,
        None,
    )
    .unwrap();

    // User B adds a URL
    let conn_b = bzod::jobs::open_user_content_conn(&db, id_b).unwrap();
    let _ = bzod::db::content::create_url_extended(
        &conn_b,
        "!slug-b",
        "https://google.com/userb",
        None,
        None,
        &vec!["tag_userb".to_string()],
        None,
        None,
        None,
    )
    .unwrap();

    // Verify User B's DB has no trace of User A's content or tags
    let url_opt = bzod::db::content::get_url_by_code(&conn_b, "!slug-a").unwrap();
    assert!(url_opt.is_none());

    let tag_exists: bool = conn_b
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM tags WHERE name = 'tag_usera');",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!tag_exists);

    let _ = fs::remove_dir_all(&temp_dir);
}
