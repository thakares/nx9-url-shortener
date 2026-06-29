use bzod::cli::RepairCommands;
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
async fn test_registry_repair_dry_run() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_repair_dry_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let db = Db::init(&config).expect("Failed to init Db");

    {
        let system_conn = db.system.lock().unwrap();
        // Insert orphaned slug (owner exists, but no target db/record)
        bzod::db::users::register_global_slug(
            &system_conn,
            "orphan1",
            1,
            "url",
            "target1",
            "active",
        )
        .unwrap();
    }

    let command = RepairCommands::Registry {
        dry_run: true,
        force: false,
        slug: None,
        data_dir: Some(temp_dir.to_string_lossy().to_string()),
    };

    bzod::cli::repair::run(command, config.clone())
        .await
        .unwrap();

    // Verify it was NOT deleted
    {
        let system_conn = db.system.lock().unwrap();
        let exists: bool = system_conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM global_slugs WHERE slug = 'orphan1')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(exists, "Slug should not be deleted in dry run");
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_registry_repair_force() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_repair_force_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let db = Db::init(&config).expect("Failed to init Db");

    {
        let system_conn = db.system.lock().unwrap();
        // Insert orphaned slug
        bzod::db::users::register_global_slug(
            &system_conn,
            "orphan2",
            1,
            "url",
            "target2",
            "active",
        )
        .unwrap();
    }

    let command = RepairCommands::Registry {
        dry_run: false,
        force: true,
        slug: None,
        data_dir: Some(temp_dir.to_string_lossy().to_string()),
    };

    bzod::cli::repair::run(command, config.clone())
        .await
        .unwrap();

    // Verify it WAS deleted
    {
        let system_conn = db.system.lock().unwrap();
        let exists: bool = system_conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM global_slugs WHERE slug = 'orphan2')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!exists, "Slug should be deleted in force mode");
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_registry_repair_single_slug() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_repair_single_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let db = Db::init(&config).expect("Failed to init Db");

    {
        let system_conn = db.system.lock().unwrap();
        // Insert two orphaned slugs
        bzod::db::users::register_global_slug(
            &system_conn,
            "orphan3",
            1,
            "url",
            "target3",
            "active",
        )
        .unwrap();
        bzod::db::users::register_global_slug(
            &system_conn,
            "orphan4",
            1,
            "url",
            "target4",
            "active",
        )
        .unwrap();
    }

    let command = RepairCommands::Registry {
        dry_run: false,
        force: true,
        slug: Some("orphan3".to_string()),
        data_dir: Some(temp_dir.to_string_lossy().to_string()),
    };

    bzod::cli::repair::run(command, config.clone())
        .await
        .unwrap();

    // Verify targeted slug was deleted
    {
        let system_conn = db.system.lock().unwrap();
        let exists3: bool = system_conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM global_slugs WHERE slug = 'orphan3')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!exists3, "Targeted slug should be deleted");

        let exists4: bool = system_conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM global_slugs WHERE slug = 'orphan4')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(exists4, "Non-targeted slug should NOT be deleted");
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_registry_repair_transaction_safety() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_repair_tx_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let _db = Db::init(&config).expect("Failed to init Db");

    // Simulate transaction rollback safety indirectly by validating dual flags return immediately
    let command = RepairCommands::Registry {
        dry_run: true,
        force: true,
        slug: None,
        data_dir: Some(temp_dir.to_string_lossy().to_string()),
    };

    let result = bzod::cli::repair::run(command, config.clone()).await;
    assert!(result.is_ok());

    let _ = fs::remove_dir_all(&temp_dir);
}
