use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::time::sleep;

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

#[tokio::test]
async fn test_upgrade_from_v0_4_0() {
    // Create a temporary directory for the legacy v0.4.0 data
    let temp_dir = std::env::temp_dir().join(format!("bzod_upgrade_test_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();

    let legacy_admin_path = temp_dir.join("admin.db");
    let legacy_system_path = temp_dir.join("system.db");
    let legacy_content_path = temp_dir.join("content.db");
    let legacy_analytics_path = temp_dir.join("analytics.db");

    // Initialize legacy databases with their respective version 1 schemas and seed data
    {
        // 1. admin.db
        let mut admin_conn = rusqlite::Connection::open(&legacy_admin_path).unwrap();
        bzod::db::migrations::run_migrations(
            &mut admin_conn,
            "admin",
            bzod::db::migrations::ADMIN_MIGRATIONS,
            None,
        )
        .unwrap();
        // Seed legacy admin user
        let legacy_admin_hash = bzod::auth::hash_password("legacy-admin-pass").unwrap();
        bzod::db::admin::create_user(&admin_conn, "admin", &legacy_admin_hash).unwrap();

        // 2. system.db
        let mut system_conn = rusqlite::Connection::open(&legacy_system_path).unwrap();
        bzod::db::migrations::run_migrations(
            &mut system_conn,
            "system",
            bzod::db::migrations::SYSTEM_MIGRATIONS,
            None,
        )
        .unwrap();

        // 3. content.db
        let mut content_conn = rusqlite::Connection::open(&legacy_content_path).unwrap();
        bzod::db::migrations::run_migrations(
            &mut content_conn,
            "content",
            bzod::db::migrations::CONTENT_MIGRATIONS,
            None,
        )
        .unwrap();
        // Seed legacy url
        let url = bzod::db::content::create_url_extended(
            &content_conn,
            "!legacy-link",
            "https://example.com/legacy-redirect-target",
            Some("Legacy URL"),
            Some("A link from v0.4.0"),
            &[],
            None,
            None,
            None,
        )
        .unwrap();

        // 4. analytics.db
        let mut analytics_conn = rusqlite::Connection::open(&legacy_analytics_path).unwrap();
        bzod::db::migrations::run_migrations(
            &mut analytics_conn,
            "analytics",
            bzod::db::migrations::ANALYTICS_MIGRATIONS,
            None,
        )
        .unwrap();
        // Seed legacy visit record matching the seeded legacy url id
        let timestamp = chrono::Utc::now().to_rfc3339();
        analytics_conn
            .execute(
                "INSERT INTO visits (id, target_type, target_id, timestamp, ip_address, user_agent, referer, accept_language, country, status_code, owner_user_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11);",
                rusqlite::params![
                    "legacy-visit-id-123",
                    "url",
                    url.id,
                    timestamp,
                    "1.1.1.1",
                    "LegacyBrowser/1.0",
                    "https://referrer-channel.com",
                    "en-US",
                    "US",
                    302,
                    1i64 // user ID 1
                ],
            )
            .unwrap();
    }

    // Now start the application setup which should trigger migrations/upgrade
    let config = create_temp_config(temp_dir.clone());
    let db = Db::init(&config).expect("Db::init failed to upgrade v0.4.0 database directory");

    // Verify file layout was reorganized
    assert!(!legacy_admin_path.exists());
    assert!(!legacy_content_path.exists());
    assert!(!legacy_analytics_path.exists());
    assert!(!legacy_system_path.exists());

    assert!(temp_dir.join("admin/admin.db").exists());
    assert!(temp_dir.join("admin/system.db").exists());
    assert!(temp_dir.join("admin/users.db").exists());
    assert!(temp_dir.join("users/1/content.db").exists());
    assert!(temp_dir.join("users/1/analytics.db").exists());

    // Spawn server
    let queue = AnalyticsQueue::new(db.clone(), 10);
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

    let router = create_router(state.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    let _server_handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    // 1. Admin login works (Bootstrap path because we migrated)
    let login_url = format!("{}/admin/login", base_url);
    let res = client.get(&login_url).send().await.unwrap();
    let html = res.text().await.unwrap();
    let csrf_token =
        extract_csrf_token(&html).expect("Failed to extract CSRF token from login page");

    let mut params = HashMap::new();
    params.insert("username", "admin");
    params.insert("password", "bootstrap-secret");
    params.insert("csrf_token", &csrf_token);

    let login_res = client.post(&login_url).form(&params).send().await.unwrap();
    assert!(login_res.status().is_redirection());
    assert_eq!(
        login_res
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap(),
        "/admin/dashboard"
    );

    // 2. Existing links redirect correctly
    let redirect_url = format!("{}/!legacy-link", base_url);
    let redirect_res = client.get(&redirect_url).send().await.unwrap();
    let res_headers = redirect_res.headers();
    assert!(res_headers.get("location").is_some());
    assert_eq!(
        res_headers.get("location").unwrap().to_str().unwrap(),
        "https://example.com/legacy-redirect-target"
    );

    // Sleep 3 seconds for the analytics queue to process and flush to database
    sleep(Duration::from_millis(3000)).await;

    // 3. Analytics preserved and new click recorded
    let user_dbs = state.get_user_dbs(1).unwrap();
    let analytics_conn = user_dbs.analytics.lock().unwrap();

    // Check count of visits for the legacy link (should be 2: 1 legacy + 1 new redirect click)
    let visit_count: i64 = analytics_conn
        .query_row(
            "SELECT COUNT(*) FROM visits WHERE target_type = 'url';",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(visit_count, 2);

    // Verify the details of the legacy visit are preserved
    let legacy_visit_exists: bool = analytics_conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM visits WHERE id = 'legacy-visit-id-123' AND user_agent = 'LegacyBrowser/1.0' AND referer = 'https://referrer-channel.com');",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(legacy_visit_exists);

    // Clean up temporary folder
    let _ = fs::remove_dir_all(&temp_dir);
}
