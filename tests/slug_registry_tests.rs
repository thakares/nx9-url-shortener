#![allow(deprecated)]

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
async fn test_admin_url_slug_collision_rejected() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_test_col_1_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let db = Db::init(&config).expect("Failed to init Db");
    let system_conn = db.system.lock().unwrap();

    bzod::db::users::register_global_slug(&system_conn, "collision", 1, "url", "url1", "active")
        .unwrap();
    let res = bzod::db::users::register_global_slug(
        &system_conn,
        "collision",
        1,
        "url",
        "url2",
        "active",
    );
    assert!(res.is_err());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_admin_page_slug_collision_rejected() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_test_col_2_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let db = Db::init(&config).expect("Failed to init Db");
    let system_conn = db.system.lock().unwrap();

    bzod::db::users::register_global_slug(&system_conn, "collision", 1, "page", "page1", "active")
        .unwrap();
    let res = bzod::db::users::register_global_slug(
        &system_conn,
        "collision",
        1,
        "page",
        "page2",
        "active",
    );
    assert!(res.is_err());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_user_url_slug_collision_rejected() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_test_col_3_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let db = Db::init(&config).expect("Failed to init Db");
    let system_conn = db.system.lock().unwrap();

    bzod::db::users::register_global_slug(&system_conn, "collision", 2, "url", "url1", "active")
        .unwrap();
    let res = bzod::db::users::register_global_slug(
        &system_conn,
        "collision",
        2,
        "url",
        "url2",
        "active",
    );
    assert!(res.is_err());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_user_page_slug_collision_rejected() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_test_col_4_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let db = Db::init(&config).expect("Failed to init Db");
    let system_conn = db.system.lock().unwrap();

    bzod::db::users::register_global_slug(&system_conn, "collision", 2, "page", "page1", "active")
        .unwrap();
    let res = bzod::db::users::register_global_slug(
        &system_conn,
        "collision",
        2,
        "page",
        "page2",
        "active",
    );
    assert!(res.is_err());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_url_page_slug_collision_rejected() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_test_col_5_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let db = Db::init(&config).expect("Failed to init Db");
    let system_conn = db.system.lock().unwrap();

    bzod::db::users::register_global_slug(&system_conn, "collision", 1, "url", "url1", "active")
        .unwrap();
    let res = bzod::db::users::register_global_slug(
        &system_conn,
        "collision",
        1,
        "page",
        "page1",
        "active",
    );
    assert!(res.is_err());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_page_url_slug_collision_rejected() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_test_col_6_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let db = Db::init(&config).expect("Failed to init Db");
    let system_conn = db.system.lock().unwrap();

    bzod::db::users::register_global_slug(&system_conn, "collision", 1, "page", "page1", "active")
        .unwrap();
    let res = bzod::db::users::register_global_slug(
        &system_conn,
        "collision",
        1,
        "url",
        "url1",
        "active",
    );
    assert!(res.is_err());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_slug_reusable_after_delete() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_test_col_7_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let db = Db::init(&config).expect("Failed to init Db");
    let system_conn = db.system.lock().unwrap();

    bzod::db::users::register_global_slug(&system_conn, "slug", 1, "url", "url1", "active")
        .unwrap();
    assert!(!bzod::db::users::is_slug_available(&system_conn, "slug").unwrap());

    bzod::db::users::release_global_slug(&system_conn, "slug", 1).unwrap();
    assert!(bzod::db::users::is_slug_available(&system_conn, "slug").unwrap());

    bzod::db::users::register_global_slug(&system_conn, "slug", 1, "page", "page1", "active")
        .unwrap();
    assert!(!bzod::db::users::is_slug_available(&system_conn, "slug").unwrap());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_disabled_slug_still_blocks_registration() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_test_col_8_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let db = Db::init(&config).expect("Failed to init Db");
    let system_conn = db.system.lock().unwrap();

    bzod::db::users::register_global_slug(&system_conn, "slug", 1, "url", "url1", "disabled")
        .unwrap();
    assert!(!bzod::db::users::is_slug_available(&system_conn, "slug").unwrap());

    let res =
        bzod::db::users::register_global_slug(&system_conn, "slug", 2, "page", "page1", "active");
    assert!(res.is_err());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_transferred_slug_remains_unique() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_test_col_9_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let db = Db::init(&config).expect("Failed to init Db");
    let system_conn = db.system.lock().unwrap();

    bzod::db::users::register_global_slug(&system_conn, "slug", 1, "url", "url1", "active")
        .unwrap();
    assert!(!bzod::db::users::is_slug_available(&system_conn, "slug").unwrap());

    // Transfer slug (by setting owner_user_id to 2)
    system_conn
        .execute(
            "UPDATE global_slugs SET owner_user_id = 2 WHERE slug = 'slug'",
            [],
        )
        .unwrap();

    let res =
        bzod::db::users::register_global_slug(&system_conn, "slug", 1, "url", "url2", "active");
    assert!(res.is_err());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_transfer_preserves_global_slug_record() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_test_col_10_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let db = Db::init(&config).expect("Failed to init Db");
    let system_conn = db.system.lock().unwrap();

    bzod::db::users::register_global_slug(&system_conn, "slug", 1, "url", "url1", "active")
        .unwrap();

    // Verify it exists in global_slugs
    let owner_id: i64 = system_conn
        .query_row(
            "SELECT owner_user_id FROM global_slugs WHERE slug = 'slug'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(owner_id, 1);

    let _ = fs::remove_dir_all(&temp_dir);
}
