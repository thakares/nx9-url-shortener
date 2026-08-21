//! v0.8.0 Phase 1: frozen database topology under the existing physical root.

use bzod::config::Config;
use bzod::db::slugs::CORE_RESERVED_SLUGS;
use bzod::db::sqlite::get_user_version;
use bzod::db::topology::{is_valid_user_dir_name, Topology};
use bzod::db::Db;
use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;

fn temp_config() -> (PathBuf, Config) {
    let temp_dir = std::env::temp_dir().join(format!("bzod_v08_topology_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let mut config = Config::load();
    config.data_dir = temp_dir.clone();
    config.backup_dir = temp_dir.join("backups");
    (temp_dir, config)
}

#[test]
fn physical_root_stays_data_dir_not_database() {
    let t = Topology::new("./data");
    assert_eq!(t.root(), std::path::Path::new("./data"));
    assert!(!t.root().ends_with("database"));
    assert!(t.slugs_dir().ends_with("data/slugs"));
}

#[tokio::test]
async fn db_init_creates_frozen_slug_topology() {
    let (temp_dir, config) = temp_config();
    let db = Db::init(&config).expect("init");

    assert!(db.topology.admin_db().exists());
    assert!(db.topology.system_db().exists());
    assert!(db.topology.users_registry_db().exists());
    assert!(db.topology.global_urls_db().exists());
    assert!(db.topology.global_landing_pages_db().exists());
    assert!(db.topology.reserved_db().exists());
    assert!(!temp_dir.join("database").exists());

    {
        let conn = db.global_urls.lock().unwrap();
        assert_eq!(
            get_user_version(&conn).unwrap(),
            bzod::db::schema_v08::GLOBAL_URLS_MIGRATIONS
                .last()
                .unwrap()
                .version
        );
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM global_urls;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "Phase 1 does not move live slugs off system.db");
    }
    {
        let conn = db.global_landing_pages.lock().unwrap();
        assert_eq!(
            get_user_version(&conn).unwrap(),
            bzod::db::schema_v08::GLOBAL_LANDING_PAGES_MIGRATIONS
                .last()
                .unwrap()
                .version
        );
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM global_landing_pages;", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(n, 0);
    }
    {
        let conn = db.reserved.lock().unwrap();
        assert_eq!(get_user_version(&conn).unwrap(), 1);
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM reserved_slugs;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, CORE_RESERVED_SLUGS.len() as i64);
        let has_admin: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM reserved_slugs WHERE slug = 'admin');",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(has_admin);
    }

    // Live slug registry is still system.db (Phase 4 moves ownership).
    {
        let conn = db.system.lock().unwrap();
        let has_table: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='global_slugs');",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(has_table);
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn backup_includes_slugs_directory() {
    let (temp_dir, config) = temp_config();
    let db = Db::init(&config).expect("init");
    let backup_path = bzod::jobs::backup::perform_backup(&db, &config)
        .await
        .expect("backup");

    let file = fs::File::open(&backup_path).unwrap();
    let dec = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(dec);
    let mut found_reserved = false;
    let mut found_global_urls = false;
    for entry in archive.entries().unwrap() {
        let entry = entry.unwrap();
        let path = entry.path().unwrap().to_string_lossy().to_string();
        if path.contains("slugs/reserved.db") {
            found_reserved = true;
        }
        if path.contains("slugs/global_urls.db") {
            found_global_urls = true;
        }
    }
    assert!(found_reserved, "backup missing slugs/reserved.db");
    assert!(found_global_urls, "backup missing slugs/global_urls.db");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn tenant_path_resolution_rejects_traversal() {
    let t = Topology::new("/app/data");
    assert!(t.user_dir("..").is_err());
    assert!(t.user_dir("../etc").is_err());
    assert!(t.content_db_i64(-1).is_err());
    assert!(is_valid_user_dir_name("2"));
    assert!(is_valid_user_dir_name("a1b2c3d4e5f6"));
    assert!(!is_valid_user_dir_name("admin"));
}

#[tokio::test]
async fn open_user_content_rejects_non_positive_id() {
    let (temp_dir, config) = temp_config();
    let db = Db::init(&config).expect("init");
    assert!(bzod::jobs::open_user_content_conn(&db, 0).is_err());
    assert!(bzod::jobs::open_user_content_conn(&db, -3).is_err());
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn restore_normalization_creates_slugs_dir() {
    let dir = std::env::temp_dir().join(format!("bzod_v08_restore_{}", uuid::Uuid::new_v4()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("admin.db"), b"a").unwrap();
    fs::write(dir.join("system.db"), b"s").unwrap();
    fs::write(dir.join("users.db"), b"u").unwrap();
    fs::write(dir.join("content.db"), b"c").unwrap();
    fs::write(dir.join("analytics.db"), b"an").unwrap();

    bzod::services::backup_layout::normalize_restored_layout(&dir).unwrap();
    assert!(dir.join("slugs").is_dir());
    assert!(Connection::open(dir.join("admin/admin.db")).is_ok());

    let _ = fs::remove_dir_all(&dir);
}
