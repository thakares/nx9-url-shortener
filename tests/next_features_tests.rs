use bzod::config::Config;
use bzod::db::Db;
use bzod::utils::validation::{validate_custom_slug, validate_page_code, validate_redirect_code};
use std::fs;
use std::path::PathBuf;

#[test]
fn test_custom_slug_validation() {
    // Valid slugs
    assert!(validate_custom_slug("!a"));
    assert!(validate_custom_slug("!home"));
    assert!(validate_custom_slug("!office"));
    assert!(validate_custom_slug("!project-ae06"));
    assert!(validate_custom_slug("!customer_01"));

    // Invalid slugs
    assert!(!validate_custom_slug("!"));
    assert!(!validate_custom_slug("!home page"));
    assert!(!validate_custom_slug("!home/page"));
    assert!(!validate_custom_slug("!home?"));
    assert!(!validate_custom_slug("!home&"));
    assert!(!validate_custom_slug("!!"));
    assert!(!validate_custom_slug(
        "!this-is-a-very-long-slug-which-exceeds-the-maximum-allowed-length-limit"
    ));

    // Redirect & page validation
    assert!(validate_redirect_code("abcdef")); // 6-hex
    assert!(validate_redirect_code("!home")); // custom slug
    assert!(!validate_redirect_code("abcde")); // invalid redirect code
    assert!(validate_page_code("abcd")); // 4-hex
    assert!(validate_page_code("!home")); // custom slug
    assert!(!validate_page_code("abc")); // invalid page code
}

fn create_temp_config(temp_dir: PathBuf) -> Config {
    let mut config = Config::load();
    config.data_dir = temp_dir.clone();
    config.backup_dir = temp_dir.clone();
    config.base_url = Some("http://bzo.in".to_string());
    config
}

#[tokio::test]
async fn test_cli_shorten_and_expand() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_test_cli_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    // 1. Shorten with generated code
    let res = bzod::cli::shorten::run(
        "https://example.com/one".to_string(),
        None,
        None,
        config.clone(),
    )
    .await;
    assert!(res.is_ok());

    // 2. Shorten with custom slug
    let res = bzod::cli::shorten::run(
        "https://example.com/two".to_string(),
        Some("!office".to_string()),
        None,
        config.clone(),
    )
    .await;
    assert!(res.is_ok());

    // 3. Shorten duplicate slug (should fail)
    let res_dup = bzod::cli::shorten::run(
        "https://example.com/three".to_string(),
        Some("!OFFICE".to_string()), // case-insensitive
        None,
        config.clone(),
    )
    .await;
    assert!(res_dup.is_err());
    assert!(res_dup.unwrap_err().to_string().contains("already exists"));

    // 4. Expand custom slug
    {
        let db = Db::init(&config).unwrap();
        let conn = db.content.lock().unwrap();
        let url_opt = bzod::db::content::get_url_by_code(&conn, "!office").unwrap();
        assert!(url_opt.is_some());
        assert_eq!(url_opt.unwrap().destination, "https://example.com/two");
    }

    // 5. CLI expand round-trip validation
    let expand_res = bzod::cli::expand::run("!office".to_string(), None, config.clone()).await;
    assert!(expand_res.is_ok());

    // 6. Case-insensitive CLI expand validation
    let expand_res_upper =
        bzod::cli::expand::run("!OFFICE".to_string(), None, config.clone()).await;
    assert!(expand_res_upper.is_ok());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_perform_restore_and_validation() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_test_restore_{}", uuid::Uuid::new_v4()));
    let restore_dir =
        std::env::temp_dir().join(format!("bzod_test_restore_dest_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    fs::create_dir_all(&restore_dir).unwrap();

    let config = create_temp_config(temp_dir.clone());
    let db = Db::init(&config).unwrap();

    // 1. Create a mock database record
    {
        let conn = db.content.lock().unwrap();
        bzod::db::content::create_url_extended(
            &conn,
            "!home",
            "https://my-home.com",
            None,
            None,
            &[],
            None,
            None,
            None,
        )
        .unwrap();
    }

    // 2. Perform a backup
    let backup_path = bzod::jobs::backup::perform_backup(&db, &config)
        .await
        .unwrap();
    assert!(PathBuf::from(&backup_path).exists());

    // 3. Validate backup archive structure
    let validation_res =
        bzod::cli::restore::perform_restore(&PathBuf::from(&backup_path), &restore_dir);
    assert!(validation_res.is_ok());

    // Verify database files were extracted
    assert!(restore_dir.join("admin.db").exists());
    assert!(restore_dir.join("content.db").exists());
    assert!(restore_dir.join("analytics.db").exists());
    assert!(restore_dir.join("system.db").exists());

    // Verify custom slug was preserved in the restored DB
    let restore_config = create_temp_config(restore_dir.clone());
    let restore_db = Db::init(&restore_config).unwrap();
    {
        let conn = restore_db.content.lock().unwrap();
        let url = bzod::db::content::get_url_by_code(&conn, "!home").unwrap();
        assert!(url.is_some());
        assert_eq!(url.unwrap().destination, "https://my-home.com");
    }

    let _ = fs::remove_dir_all(&temp_dir);
    let _ = fs::remove_dir_all(&restore_dir);
}
