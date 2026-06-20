use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum_extra::extract::CookieJar;
use bzod::analytics::AnalyticsQueue;
use bzod::config::Config;
use bzod::db::Db;
use bzod::state::AppState;
use bzod::web::redirect::resolve_redirect;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

fn create_temp_config(temp_dir: PathBuf) -> Config {
    let mut config = Config::load();
    config.data_dir = temp_dir.clone();
    config.backup_dir = temp_dir.clone();
    config.base_url = Some("http://bzo.in".to_string());
    config
}

#[tokio::test]
async fn test_global_slug_lookup_and_redirection() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_test_routing_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");
    let queue = AnalyticsQueue::new(db.clone(), 1000);

    let state = AppState {
        admin_db: db.admin.clone(),
        content_db: db.content.clone(),
        analytics_db: db.analytics.clone(),
        system_db: db.system.clone(),
        users_db: db.users.clone(),
        user_dbs: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        db: db.clone(),
        config: config.clone(),
        analytics_queue: queue,
        start_time: Instant::now(),
    };

    // Create a user and link
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

    {
        let conn = bzod::jobs::open_user_content_conn(&db, user_id).unwrap();
        bzod::db::content::create_url_extended(
            &conn,
            "!my-routing-slug",
            "https://example.com/target",
            None,
            None,
            &vec![],
            None,
            None,
            None,
        )
        .unwrap();

        let system_conn = db.system.lock().unwrap();
        bzod::db::users::register_global_slug(
            &system_conn,
            "!my-routing-slug",
            user_id,
            "url",
            "xyz",
            "active",
        )
        .unwrap();
    }

    // Call resolve_redirect directly
    let response = resolve_redirect(
        State(state.clone()),
        CookieJar::new(),
        Path("!my-routing-slug".to_string()),
        HeaderMap::new(),
        None,
    )
    .await;

    // Verify it redirects (status 303 or 302/307/etc)
    assert!(response.status().is_redirection());
    assert_eq!(
        response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap(),
        "https://example.com/target"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_disabled_slug_returns_410() {
    let temp_dir = std::env::temp_dir().join(format!(
        "bzod_test_routing_disabled_{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Failed to init Db");
    let queue = AnalyticsQueue::new(db.clone(), 1000);

    let state = AppState {
        admin_db: db.admin.clone(),
        content_db: db.content.clone(),
        analytics_db: db.analytics.clone(),
        system_db: db.system.clone(),
        users_db: db.users.clone(),
        user_dbs: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        db: db.clone(),
        config: config.clone(),
        analytics_queue: queue,
        start_time: Instant::now(),
    };

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

    {
        let conn = bzod::jobs::open_user_content_conn(&db, user_id).unwrap();
        bzod::db::content::create_url_extended(
            &conn,
            "!disabled-slug",
            "https://example.com/target",
            None,
            None,
            &vec![],
            None,
            None,
            None,
        )
        .unwrap();

        let system_conn = db.system.lock().unwrap();
        bzod::db::users::register_global_slug(
            &system_conn,
            "!disabled-slug",
            user_id,
            "url",
            "xyz",
            "active",
        )
        .unwrap();

        // Disable slug
        system_conn
            .execute(
                "UPDATE global_slugs SET status = 'disabled' WHERE slug = '!disabled-slug';",
                [],
            )
            .unwrap();
    }

    // Call resolve_redirect directly
    let response = resolve_redirect(
        State(state),
        CookieJar::new(),
        Path("!disabled-slug".to_string()),
        HeaderMap::new(),
        None,
    )
    .await;

    // Verify it returns 410 Gone (StatusCode::GONE)
    assert_eq!(response.status(), StatusCode::GONE);

    let _ = fs::remove_dir_all(&temp_dir);
}
