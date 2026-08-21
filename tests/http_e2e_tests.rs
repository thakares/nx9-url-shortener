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
    // We leave config.cookie_secure as default (true).
    // The new resolve_cookie_secure logic will automatically drop Secure
    // for HTTP requests over the 127.0.0.1 loopback during this test,
    // proving the local development fix works end-to-end.
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

// Start a test server on a random free port and return client + address
async fn start_test_server(
    temp_dir: PathBuf,
) -> (reqwest::Client, String, tokio::task::JoinHandle<()>) {
    let config = create_temp_config(temp_dir);
    let db = Db::init(&config).expect("Failed to init Db");
    let (tx, rx) = tokio::sync::watch::channel(false);
    Box::leak(Box::new(tx));
    let (queue, _) = AnalyticsQueue::new(db.clone(), 100, rx);

    let state = AppState {
        admin_db: db.admin.clone(),
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
        .redirect(reqwest::redirect::Policy::none()) // Let us manually inspect redirects
        .build()
        .unwrap();

    (client, url, handle)
}

#[tokio::test]
async fn test_full_http_e2e_flow() {
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_e2e_http_test_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();

    let (client, base_url, _server_handle) = start_test_server(temp_dir.clone()).await;

    // 1. Fetch admin login page to get CSRF token
    let login_page_url = format!("{}/admin/login", base_url);
    let res = client.get(&login_page_url).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);

    let html = res.text().await.unwrap();
    let csrf_token = extract_csrf_token(&html).expect("Failed to extract CSRF token");

    // 2. Perform bootstrap login using BOOTSTRAP_PASSWORD_SHA256
    let mut params = HashMap::new();
    params.insert("username", "admin");
    params.insert("password", "bootstrap-secret");
    params.insert("csrf_token", &csrf_token);

    let res = client
        .post(&login_page_url)
        .form(&params)
        .send()
        .await
        .unwrap();

    // After successful login, it should redirect to /admin/dashboard
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);
    let redirect_url = res.headers().get("location").unwrap().to_str().unwrap();
    assert_eq!(redirect_url, "/admin/dashboard");

    // 3. Try to access /admin/users to make sure we are authorized
    let users_page_url = format!("{}/admin/users", base_url);
    let res = client.get(&users_page_url).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);

    let html = res.text().await.unwrap();
    assert!(html.contains("Users Management"));

    // Extract CSRF token from admin users page
    let csrf_token =
        extract_csrf_token(&html).expect("Failed to extract CSRF token from users page");

    // 4. Test CSRF validation failure during user creation
    let mut bad_params = HashMap::new();
    bad_params.insert("username", "testuser");
    bad_params.insert("password", "password123");
    bad_params.insert("account_type", "standard");
    bad_params.insert("metadata", "e2e standard user");
    bad_params.insert("csrf_token", "invalid-csrf-token");

    let create_user_url = format!("{}/admin/users/create", base_url);
    let res = client
        .post(&create_user_url)
        .form(&bad_params)
        .send()
        .await
        .unwrap();

    // Should redirect back with Invalid CSRF token error
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);
    let redirect_url = res.headers().get("location").unwrap().to_str().unwrap();
    assert!(redirect_url.contains("Invalid CSRF token"));

    // 5. Create a standard user successfully
    let mut good_params = HashMap::new();
    good_params.insert("username", "testuser");
    good_params.insert("password", "password123");
    good_params.insert("account_type", "standard");
    good_params.insert("metadata", "e2e standard user");
    good_params.insert("csrf_token", &csrf_token);

    let res = client
        .post(&create_user_url)
        .form(&good_params)
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);
    let redirect_url = res.headers().get("location").unwrap().to_str().unwrap();
    assert!(redirect_url.contains("success=User created successfully"));

    // 6. Test RBAC: logout admin and login as standard user
    let admin_logout_url = format!("{}/admin/logout", base_url);
    let res = client.get(&admin_logout_url).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);

    // Get public login page and extract CSRF token
    let public_login_page_url = format!("{}/login", base_url);
    let res = client.get(&public_login_page_url).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);

    let html = res.text().await.unwrap();
    let csrf_token = extract_csrf_token(&html).expect("Failed to extract public CSRF token");

    // Login as standard user
    let mut user_login_params = HashMap::new();
    user_login_params.insert("username", "testuser");
    user_login_params.insert("password", "password123");
    user_login_params.insert("csrf_token", &csrf_token);

    let res = client
        .post(&public_login_page_url)
        .form(&user_login_params)
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);
    let redirect_url = res.headers().get("location").unwrap().to_str().unwrap();
    assert_eq!(redirect_url, "/user/dashboard");

    // Access user dashboard (should succeed)
    let user_dashboard_url = format!("{}/user/dashboard", base_url);
    let res = client.get(&user_dashboard_url).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);

    // Try to access admin dashboard as standard user (RBAC check - should return 403 Forbidden)
    let admin_dashboard_url = format!("{}/admin/dashboard", base_url);
    let res = client.get(&admin_dashboard_url).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::FORBIDDEN);

    let _ = fs::remove_dir_all(&temp_dir);
}
