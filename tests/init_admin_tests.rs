use bzod::config::Config;
use bzod::db::Db;
use std::env;
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
async fn test_init_admin() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_test_init_admin_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    // Case C/D - missing env vars
    env::remove_var("ADMIN_USERNAME");
    env::remove_var("ADMIN_PASSWORD");
    let res = bzod::cli::init_admin::run(None, config.clone()).await;
    assert!(res.is_err(), "Should fail without env vars");

    // Case A - No admin + valid ENV
    env::set_var("ADMIN_USERNAME", "admin");
    env::set_var("ADMIN_PASSWORD", "securepass");
    let res = bzod::cli::init_admin::run(None, config.clone()).await;
    assert!(res.is_ok(), "Should succeed with valid env vars");

    let db = Db::init(&config).unwrap();
    {
        let conn = db.users.lock().unwrap();
        let user = bzod::db::users::get_user_by_username(&conn, "admin")
            .unwrap()
            .unwrap();
        assert_eq!(user.account_type, "admin");
        assert!(bzod::auth::verify_password(
            "securepass",
            &user.password_hash
        ));
    }

    // Case B/F - Admin already exists
    env::remove_var("ADMIN_USERNAME");
    env::remove_var("ADMIN_PASSWORD");
    let res = bzod::cli::init_admin::run(None, config.clone()).await;
    assert!(
        res.is_ok(),
        "Should skip and succeed if admin exists even without env vars"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}
