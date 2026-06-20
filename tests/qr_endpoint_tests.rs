use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use tokio::net::TcpListener;

use bzod::analytics::AnalyticsQueue;
use bzod::config::Config;
use bzod::db::Db;
use bzod::state::AppState;
use bzod::web::create_router;

fn compute_sha256(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

fn create_temp_config(temp_dir: PathBuf) -> Config {
    let mut config = Config::load();
    config.data_dir = temp_dir.clone();
    config.backup_dir = temp_dir.clone();
    config.admin_username = "admin".to_string();
    config.base_url = Some("http://localhost:8080".to_string());
    config.cookie_secure = false;
    config.bootstrap_password_sha256 = compute_sha256("bootstrap-secret");
    config
}

async fn start_test_server(
    temp_dir: PathBuf,
) -> (reqwest::Client, String, tokio::task::JoinHandle<()>, Db) {
    let config = create_temp_config(temp_dir);
    let db = Db::init(&config).expect("Failed to init Db");
    let queue = AnalyticsQueue::new(db.clone(), 100);

    let state = AppState {
        admin_db: db.admin.clone(),
        content_db: db.content.clone(),
        analytics_db: db.analytics.clone(),
        system_db: db.system.clone(),
        users_db: db.users.clone(),
        user_dbs: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
        db: db.clone(),
        config,
        analytics_queue: queue,
        start_time: Instant::now(),
    };

    let router = create_router(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);

    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    (client, url, handle, db)
}

#[tokio::test]
async fn test_qr_endpoints_and_redirection_hardening() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_qr_test_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();

    let (client, base_url, _server_handle, db) = start_test_server(temp_dir.clone()).await;

    // Create User A
    let _ = bzod::cli::create_user::run(
        Some("usera".to_string()),
        Some("password123".to_string()),
        None,
        create_temp_config(temp_dir.clone()),
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

    // User A adds an active URL
    {
        let conn_a = bzod::jobs::open_user_content_conn(&db, id_a).unwrap();
        let url = bzod::db::content::create_url_extended(
            &conn_a,
            "a1b2c3",
            "https://active.com",
            Some("active-url-title"),
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
            "a1b2c3",
            id_a,
            "url",
            &url.id,
            "active",
        )
        .unwrap();
    }

    // User A adds a disabled URL
    {
        let conn_a = bzod::jobs::open_user_content_conn(&db, id_a).unwrap();
        let url = bzod::db::content::create_url_extended(
            &conn_a,
            "d4e5f6",
            "https://disabled.com",
            Some("disabled-url-title"),
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
            "d4e5f6",
            id_a,
            "url",
            &url.id,
            "disabled",
        )
        .unwrap();
    }

    // User A adds an active Page
    {
        let conn_a = bzod::jobs::open_user_content_conn(&db, id_a).unwrap();
        let page = bzod::db::content::create_landing_page(
            &conn_a,
            "a1b2",
            "active-page",
            "Active Page",
            "<html></html>",
            "published",
        )
        .unwrap();

        let system_conn = db.system.lock().unwrap();
        bzod::db::users::register_global_slug(
            &system_conn,
            "a1b2",
            id_a,
            "page",
            &page.id,
            "active",
        )
        .unwrap();
    }

    // User A adds a disabled Page
    {
        let conn_a = bzod::jobs::open_user_content_conn(&db, id_a).unwrap();
        let page = bzod::db::content::create_landing_page(
            &conn_a,
            "c3d4",
            "disabled-page",
            "Disabled Page",
            "<html></html>",
            "draft",
        )
        .unwrap();

        let system_conn = db.system.lock().unwrap();
        bzod::db::users::register_global_slug(
            &system_conn,
            "c3d4",
            id_a,
            "page",
            &page.id,
            "disabled",
        )
        .unwrap();
    }

    // 1. Verify Active URL QR endpoints return 200 with correct content types
    let res = client
        .get(format!("{}/api/qr/a1b2c3.png", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    assert_eq!(
        res.headers().get("content-type").unwrap().to_str().unwrap(),
        "image/png"
    );

    let res = client
        .get(format!("{}/api/qr/a1b2c3.svg", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    assert_eq!(
        res.headers().get("content-type").unwrap().to_str().unwrap(),
        "image/svg+xml"
    );

    // 2. Verify Disabled URL redirect returns 410 Gone
    let res = client
        .get(format!("{}/a1b2c3", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::MOVED_PERMANENTLY); // Redirects to destination

    let res = client
        .get(format!("{}/d4e5f6", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::GONE);

    // 3. Verify Disabled URL QR endpoints return 410 Gone
    let res = client
        .get(format!("{}/api/qr/d4e5f6.png", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::GONE);

    let res = client
        .get(format!("{}/api/qr/d4e5f6.svg", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::GONE);

    // 4. Verify Active Page QR endpoints return 200
    let res = client
        .get(format!("{}/api/qr/a1b2.png", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);

    let res = client
        .get(format!("{}/api/qr/a1b2.svg", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);

    // 5. Verify Disabled Page redirection returns 410 Gone
    let res = client
        .get(format!("{}/p/c3d4", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::GONE);

    // 6. Verify Disabled Page QR endpoints return 410 Gone
    let res = client
        .get(format!("{}/api/qr/c3d4.png", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::GONE);

    let res = client
        .get(format!("{}/api/qr/c3d4.svg", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::GONE);

    // 7. Verify Missing URL/Page QR endpoints return 404 Not Found
    let res = client
        .get(format!("{}/api/qr/missing-slug.png", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::NOT_FOUND);

    let _ = fs::remove_dir_all(&temp_dir);
}
