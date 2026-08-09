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

fn extract_csrf_token(html: &str) -> Option<String> {
    let marker = "name=\"csrf_token\" value=\"";
    if let Some(pos) = html.find(marker) {
        let start = pos + marker.len();
        if let Some(end) = html[start..].find('"') {
            return Some(html[start..start + end].to_string());
        }
    }
    None
}

async fn start_test_server(
    temp_dir: PathBuf,
) -> (reqwest::Client, String, tokio::task::JoinHandle<()>, Db) {
    let config = create_temp_config(temp_dir);
    let db = Db::init(&config).expect("Failed to init Db");
    let (tx, rx) = tokio::sync::watch::channel(false);
    Box::leak(Box::new(tx));
    let (queue, _) = AnalyticsQueue::new(db.clone(), 100, rx);

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
async fn test_analytics_and_table_parity() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_parity_test_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();

    let (client, base_url, _server_handle, db) = start_test_server(temp_dir.clone()).await;

    // 1. Log in as admin FIRST (Bootstrap phase: zero users exist)
    let admin_client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let admin_login_url = format!("{}/admin/login", base_url);
    let res = admin_client.get(&admin_login_url).send().await.unwrap();
    let html = res.text().await.unwrap();
    let csrf_token = extract_csrf_token(&html).unwrap();

    let mut admin_login_params = HashMap::new();
    admin_login_params.insert("username", "admin");
    admin_login_params.insert("password", "bootstrap-secret");
    admin_login_params.insert("csrf_token", &csrf_token);

    let res = admin_client
        .post(&admin_login_url)
        .form(&admin_login_params)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);

    // Admin adds a URL to the global content_db
    let _admin_url_id = {
        let conn_admin = db.content.lock().unwrap();
        let url = bzod::db::content::create_url_extended(
            &conn_admin,
            "!admin-slug",
            "https://admin.com",
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
            "!admin-slug",
            1,
            "url",
            &url.id,
            "active",
        )
        .unwrap();
        url.id
    };

    // Admin adds a Landing Page to the global content_db
    let _admin_page_id = {
        let conn_admin = db.content.lock().unwrap();
        let page = bzod::db::content::create_landing_page(
            &conn_admin,
            "!admin-page",
            "admin-page",
            "Title Admin",
            "<html></html>",
            "published",
        )
        .unwrap();

        let system_conn = db.system.lock().unwrap();
        bzod::db::users::register_global_slug(
            &system_conn,
            "!admin-page",
            1,
            "page",
            &page.id,
            "active",
        )
        .unwrap();
        page.id
    };

    // 2. Create User A
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

    // User A adds a URL
    let urla_id = {
        let conn_a = bzod::jobs::open_user_content_conn(&db, id_a).unwrap();
        let url = bzod::db::content::create_url_extended(
            &conn_a,
            "!usera-slug",
            "https://usera.com",
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
            "!usera-slug",
            id_a,
            "url",
            &url.id,
            "active",
        )
        .unwrap();
        url.id
    };

    // User A adds a Landing Page
    let pagea_id = {
        let conn_a = bzod::jobs::open_user_content_conn(&db, id_a).unwrap();
        let page = bzod::db::content::create_landing_page(
            &conn_a,
            "!usera-page",
            "usera-page",
            "Title A",
            "<html></html>",
            "published",
        )
        .unwrap();

        let system_conn = db.system.lock().unwrap();
        bzod::db::users::register_global_slug(
            &system_conn,
            "!usera-page",
            id_a,
            "page",
            &page.id,
            "active",
        )
        .unwrap();
        page.id
    };

    // --- Log in as User A ---
    let login_url = format!("{}/login", base_url);
    let res = client.get(&login_url).send().await.unwrap();
    let html = res.text().await.unwrap();
    let csrf_token = extract_csrf_token(&html).unwrap();

    let mut login_params = HashMap::new();
    login_params.insert("username", "usera");
    login_params.insert("password", "password123");
    login_params.insert("csrf_token", &csrf_token);

    let res = client
        .post(&login_url)
        .form(&login_params)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);

    // 1. Get user URLs dashboard and check for QR Code preview
    let res = client
        .get(format!("{}/user/urls", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let user_urls_html = res.text().await.unwrap();
    assert!(user_urls_html.contains("QR Code"));
    assert!(user_urls_html.contains("/api/qr/!usera-slug.svg"));

    // 2. Get user Pages dashboard and check for QR Code preview
    let res = client
        .get(format!("{}/user/pages", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let user_pages_html = res.text().await.unwrap();
    assert!(user_pages_html.contains("QR Code"));
    assert!(user_pages_html.contains("/api/qr/!usera-page.svg"));

    // 3. Get user URL analytics and verify dashboard features exist
    let res = client
        .get(format!("{}/user/analytics/url/{}", base_url, urla_id))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let user_url_analytics = res.text().await.unwrap();
    assert!(user_url_analytics.contains("Export CSV"));
    assert!(user_url_analytics.contains("Export JSON"));
    assert!(user_url_analytics.contains("date_from"));
    assert!(user_url_analytics.contains("Daily Click Traffic"));
    assert!(user_url_analytics.contains("Referrer Channels"));
    assert!(user_url_analytics.contains("Browser breakdown"));

    // 4. Get user Page analytics and verify dashboard features exist
    let res = client
        .get(format!("{}/user/analytics/page/{}", base_url, pagea_id))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let user_page_analytics = res.text().await.unwrap();
    assert!(user_page_analytics.contains("Export CSV"));
    assert!(user_page_analytics.contains("Export JSON"));
    assert!(user_page_analytics.contains("date_from"));
    assert!(user_page_analytics.contains("Daily Page Views"));
    assert!(user_page_analytics.contains("Referrer Channels"));

    // 5. Get admin URLs dashboard and check for QR Code preview
    let res = admin_client
        .get(format!("{}/admin/urls", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let admin_urls_html = res.text().await.unwrap();
    assert!(admin_urls_html.contains("QR Code"));
    assert!(admin_urls_html.contains("/api/qr/!admin-slug.svg"));

    // 6. Get admin Pages dashboard and check for QR Code preview
    let res = admin_client
        .get(format!("{}/admin/pages", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let admin_pages_html = res.text().await.unwrap();
    assert!(admin_pages_html.contains("QR Code"));
    assert!(admin_pages_html.contains("/api/qr/!admin-page.svg"));

    let _ = fs::remove_dir_all(&temp_dir);
}
