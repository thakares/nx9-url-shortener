use bzod::config::Config;
use bzod::db::Db;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Barrier;

fn create_temp_config(temp_dir: PathBuf) -> Config {
    let mut config = Config::load();
    config.data_dir = temp_dir.clone();
    config.backup_dir = temp_dir.clone();
    config.base_url = Some("http://bzo.in".to_string());
    config
}

#[tokio::test]
async fn test_concurrent_slug_creation() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_concurrency_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");

    let barrier = Arc::new(Barrier::new(2));
    let db_path = temp_dir.join("admin/system.db");

    let b1 = barrier.clone();
    let path1 = db_path.clone();
    let task1 = tokio::spawn(async move {
        let conn = rusqlite::Connection::open(&path1).unwrap();
        b1.wait().await;
        bzod::db::users::register_global_slug(&conn, "!conc-slug", 10, "url", "url10")
    });

    let b2 = barrier.clone();
    let path2 = db_path.clone();
    let task2 = tokio::spawn(async move {
        let conn = rusqlite::Connection::open(&path2).unwrap();
        b2.wait().await;
        bzod::db::users::register_global_slug(&conn, "!conc-slug", 20, "url", "url20")
    });

    let res1 = task1.await.unwrap();
    let res2 = task2.await.unwrap();

    // Exactly one should succeed, and one should fail (due to UNIQUE constraint)
    match (res1, res2) {
        (Ok(_), Err(_)) => {}
        (Err(_), Ok(_)) => {}
        (r1, r2) => panic!(
            "Concurrent creation outcome invalid. Res1: {:?}, Res2: {:?}",
            r1, r2
        ),
    }

    // Verify exactly 1 record exists in global_slugs
    {
        let system_conn = db.system.lock().unwrap();
        let count: i64 = system_conn
            .query_row(
                "SELECT COUNT(*) FROM global_slugs WHERE slug = '!conc-slug';",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    let _ = fs::remove_dir_all(&temp_dir);
}
