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
async fn test_ownership_isolation_endpoints() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_ownership_test_{}", uuid::Uuid::new_v4()));
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

    // Create User B
    let _ = bzod::cli::create_user::run(
        Some("userb".to_string()),
        Some("password123".to_string()),
        None,
        create_temp_config(temp_dir.clone()),
    )
    .await
    .unwrap();

    let (id_a, _id_b) = {
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

    // 1. Log in as User B
    let login_url = format!("{}/login", base_url);
    let res = client.get(&login_url).send().await.unwrap();
    let html = res.text().await.unwrap();
    let csrf_token = extract_csrf_token(&html).unwrap();

    let mut login_params = HashMap::new();
    login_params.insert("username", "userb");
    login_params.insert("password", "password123");
    login_params.insert("csrf_token", &csrf_token);

    let res = client
        .post(&login_url)
        .form(&login_params)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER); // Redirects after login

    // 2. User B tries to view User A's URL analytics -> 403 Forbidden
    let url_analytics_url = format!("{}/user/analytics/url/{}", base_url, urla_id);
    let res = client.get(&url_analytics_url).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::FORBIDDEN);

    // 3. User B tries to view User A's Page analytics -> 403 Forbidden
    let page_analytics_url = format!("{}/user/analytics/page/{}", base_url, pagea_id);
    let res = client.get(&page_analytics_url).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::FORBIDDEN);

    // 4. User B tries to export CSV of User A's URL analytics -> 403 Forbidden
    let csv_export_url = format!("{}/user/analytics/url/{}/export/csv", base_url, urla_id);
    let res = client.get(&csv_export_url).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::FORBIDDEN);

    // 5. User B tries to export JSON of User A's URL analytics -> 403 Forbidden
    let json_export_url = format!("{}/user/analytics/url/{}/export/json", base_url, urla_id);
    let res = client.get(&json_export_url).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::FORBIDDEN);

    // 6. User B tries to export CSV of User A's Page analytics -> 403 Forbidden
    let csv_page_export_url = format!("{}/user/analytics/page/{}/export/csv", base_url, pagea_id);
    let res = client.get(&csv_page_export_url).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::FORBIDDEN);

    // 7. User B tries to export JSON of User A's Page analytics -> 403 Forbidden
    let json_page_export_url = format!("{}/user/analytics/page/{}/export/json", base_url, pagea_id);
    let res = client.get(&json_page_export_url).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::FORBIDDEN);

    let _ = fs::remove_dir_all(&temp_dir);
}
