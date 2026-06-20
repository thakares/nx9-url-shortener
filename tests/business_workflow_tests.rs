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
) -> (
    reqwest::Client,
    String,
    AppState,
    tokio::task::JoinHandle<()>,
) {
    let config = create_temp_config(temp_dir);
    let db = Db::init(&config).expect("Failed to init Db");
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
    let url = format!("http://{}", addr);

    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    (client, url, state, handle)
}

#[tokio::test]
async fn test_scenario_a_user_create_login_shorten_visit_analytics() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_biz_test_a_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();

    let (client, base_url, state, _server_handle) = start_test_server(temp_dir.clone()).await;

    // 1. Login admin
    let login_page_url = format!("{}/admin/login", base_url);
    let res = client.get(&login_page_url).send().await.unwrap();
    let csrf_token = extract_csrf_token(&res.text().await.unwrap()).unwrap();

    let mut params = HashMap::new();
    params.insert("username", "admin");
    params.insert("password", "bootstrap-secret");
    params.insert("csrf_token", &csrf_token);
    client
        .post(&login_page_url)
        .form(&params)
        .send()
        .await
        .unwrap();

    // 2. Create user standard_a
    let users_page_url = format!("{}/admin/users", base_url);
    let res = client.get(&users_page_url).send().await.unwrap();
    let csrf_token = extract_csrf_token(&res.text().await.unwrap()).unwrap();

    let mut good_params = HashMap::new();
    good_params.insert("username", "standard_a");
    good_params.insert("password", "password123");
    good_params.insert("account_type", "standard");
    good_params.insert("metadata", "scenario a user");
    good_params.insert("csrf_token", &csrf_token);
    client
        .post(format!("{}/admin/users/create", base_url))
        .form(&good_params)
        .send()
        .await
        .unwrap();

    // Logout admin
    client
        .get(format!("{}/admin/logout", base_url))
        .send()
        .await
        .unwrap();

    // 3. Login standard_a
    let public_login_page_url = format!("{}/login", base_url);
    let res = client.get(&public_login_page_url).send().await.unwrap();
    let csrf_token = extract_csrf_token(&res.text().await.unwrap()).unwrap();

    let mut user_login_params = HashMap::new();
    user_login_params.insert("username", "standard_a");
    user_login_params.insert("password", "password123");
    user_login_params.insert("csrf_token", &csrf_token);
    client
        .post(&public_login_page_url)
        .form(&user_login_params)
        .send()
        .await
        .unwrap();

    // 4. Create a shortened URL
    let user_urls_url = format!("{}/user/urls", base_url);
    let res = client.get(&user_urls_url).send().await.unwrap();
    let csrf_token = extract_csrf_token(&res.text().await.unwrap()).unwrap();

    let mut create_url_params = HashMap::new();
    create_url_params.insert("destination", "https://google.com");
    create_url_params.insert("code", "");
    create_url_params.insert("custom_slug", "!mygoogle");
    create_url_params.insert("title", "Google");
    create_url_params.insert("description", "Search Engine");
    create_url_params.insert("tags", "search,google");
    create_url_params.insert("csrf_token", &csrf_token);
    create_url_params.insert("expires_at", "");
    create_url_params.insert("password", "");
    create_url_params.insert("max_access_count", "");
    create_url_params.insert("utm_source", "");
    create_url_params.insert("utm_medium", "");
    create_url_params.insert("utm_campaign", "");

    client
        .post(format!("{}/user/urls/create", base_url))
        .form(&create_url_params)
        .send()
        .await
        .unwrap();

    // 5. Perform redirection visit to the custom slug
    let redir_url = format!("{}/!mygoogle", base_url);
    let res = client.get(&redir_url).send().await.unwrap();
    // It should redirect to google.com
    assert_eq!(res.status(), reqwest::StatusCode::MOVED_PERMANENTLY);
    assert_eq!(
        res.headers().get("location").unwrap().to_str().unwrap(),
        "https://google.com"
    );

    // Flush the queue manually or wait a moment for the queue to write to database
    tokio::time::sleep(tokio::time::Duration::from_millis(2500)).await;

    // 6. Verify visit is logged in analytics
    let url_uuid = {
        let conn = state.system_db.lock().unwrap();
        conn.query_row(
            "SELECT target_id FROM global_slugs WHERE slug = '!mygoogle';",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
    };

    let res = client
        .get(format!("{}/analytics", base_url))
        .send()
        .await
        .unwrap();
    let html = res.text().await.unwrap();
    assert!(html.contains(&url_uuid));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_scenario_b_user_disable_session_invalidation() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_biz_test_b_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();

    let (client, base_url, state, _server_handle) = start_test_server(temp_dir.clone()).await;

    // 1. Login admin
    let login_page_url = format!("{}/admin/login", base_url);
    let res = client.get(&login_page_url).send().await.unwrap();
    let csrf_token = extract_csrf_token(&res.text().await.unwrap()).unwrap();

    let mut params = HashMap::new();
    params.insert("username", "admin");
    params.insert("password", "bootstrap-secret");
    params.insert("csrf_token", &csrf_token);
    client
        .post(&login_page_url)
        .form(&params)
        .send()
        .await
        .unwrap();

    // 2. Create user standard_b
    let users_page_url = format!("{}/admin/users", base_url);
    let res = client.get(&users_page_url).send().await.unwrap();
    let csrf_token = extract_csrf_token(&res.text().await.unwrap()).unwrap();

    let mut good_params = HashMap::new();
    good_params.insert("username", "standard_b");
    good_params.insert("password", "password123");
    good_params.insert("account_type", "standard");
    good_params.insert("metadata", "scenario b user");
    good_params.insert("csrf_token", &csrf_token);
    client
        .post(format!("{}/admin/users/create", base_url))
        .form(&good_params)
        .send()
        .await
        .unwrap();

    let target_user_id = {
        let conn = state.users_db.lock().unwrap();
        conn.query_row(
            "SELECT id FROM users WHERE username = 'standard_b';",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
    };

    // Logout admin
    client
        .get(format!("{}/admin/logout", base_url))
        .send()
        .await
        .unwrap();

    // 3. Login standard_b
    let public_login_page_url = format!("{}/login", base_url);
    let res = client.get(&public_login_page_url).send().await.unwrap();
    let csrf_token = extract_csrf_token(&res.text().await.unwrap()).unwrap();

    let mut user_login_params = HashMap::new();
    user_login_params.insert("username", "standard_b");
    user_login_params.insert("password", "password123");
    user_login_params.insert("csrf_token", &csrf_token);
    client
        .post(&public_login_page_url)
        .form(&user_login_params)
        .send()
        .await
        .unwrap();

    // Verify standard_b can access dashboard
    let res = client
        .get(format!("{}/user/dashboard", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);

    // 4. Create another client for admin to disable standard_b
    let admin_client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    // Login admin
    let res = admin_client.get(&login_page_url).send().await.unwrap();
    let csrf_token = extract_csrf_token(&res.text().await.unwrap()).unwrap();
    let mut admin_params = HashMap::new();
    admin_params.insert("username", "admin");
    admin_params.insert("password", "bootstrap-secret");
    admin_params.insert("csrf_token", &csrf_token);
    admin_client
        .post(&login_page_url)
        .form(&admin_params)
        .send()
        .await
        .unwrap();

    // GET /admin/users to get token
    let res = admin_client
        .get(format!("{}/admin/users", base_url))
        .send()
        .await
        .unwrap();
    let csrf_token = extract_csrf_token(&res.text().await.unwrap()).unwrap();

    // Disable standard_b
    let mut disable_params = HashMap::new();
    disable_params.insert("status", "disabled");
    disable_params.insert("csrf_token", &csrf_token);
    let disable_url = format!("{}/admin/users/status/{}", base_url, target_user_id);
    admin_client
        .post(&disable_url)
        .form(&disable_params)
        .send()
        .await
        .unwrap();

    // 5. Access user dashboard with standard_b's client (should be rejected/redirected to login)
    let res = client
        .get(format!("{}/user/dashboard", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);
    assert_eq!(
        res.headers().get("location").unwrap().to_str().unwrap(),
        "/login"
    );

    // Attempting login again should fail
    let res = client.get(&public_login_page_url).send().await.unwrap();
    let csrf_token = extract_csrf_token(&res.text().await.unwrap()).unwrap();
    let mut login_again = HashMap::new();
    login_again.insert("username", "standard_b");
    login_again.insert("password", "password123");
    login_again.insert("csrf_token", &csrf_token);
    let res = client
        .post(&public_login_page_url)
        .form(&login_again)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);
    assert!(res
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("error=Invalid"));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_scenario_c_slug_transfer_workflow() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_biz_test_c_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();

    let (client, base_url, state, _server_handle) = start_test_server(temp_dir.clone()).await;

    // 1. Login admin
    let login_page_url = format!("{}/admin/login", base_url);
    let res = client.get(&login_page_url).send().await.unwrap();
    let csrf_token = extract_csrf_token(&res.text().await.unwrap()).unwrap();

    let mut params = HashMap::new();
    params.insert("username", "admin");
    params.insert("password", "bootstrap-secret");
    params.insert("csrf_token", &csrf_token);
    client
        .post(&login_page_url)
        .form(&params)
        .send()
        .await
        .unwrap();

    // 2. Create standard_c1 and standard_c2
    let res = client
        .get(format!("{}/admin/users", base_url))
        .send()
        .await
        .unwrap();
    let csrf_token = extract_csrf_token(&res.text().await.unwrap()).unwrap();

    let mut user1_params = HashMap::new();
    user1_params.insert("username", "standard_c1");
    user1_params.insert("password", "password123");
    user1_params.insert("account_type", "standard");
    user1_params.insert("metadata", "c1 user");
    user1_params.insert("csrf_token", &csrf_token);
    client
        .post(format!("{}/admin/users/create", base_url))
        .form(&user1_params)
        .send()
        .await
        .unwrap();

    let res = client
        .get(format!("{}/admin/users", base_url))
        .send()
        .await
        .unwrap();
    let csrf_token = extract_csrf_token(&res.text().await.unwrap()).unwrap();

    let mut user2_params = HashMap::new();
    user2_params.insert("username", "standard_c2");
    user2_params.insert("password", "password123");
    user2_params.insert("account_type", "standard");
    user2_params.insert("metadata", "c2 user");
    user2_params.insert("csrf_token", &csrf_token);
    client
        .post(format!("{}/admin/users/create", base_url))
        .form(&user2_params)
        .send()
        .await
        .unwrap();

    let user2_id = {
        let conn = state.users_db.lock().unwrap();
        conn.query_row(
            "SELECT id FROM users WHERE username = 'standard_c2';",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
    };

    // Logout admin
    client
        .get(format!("{}/admin/logout", base_url))
        .send()
        .await
        .unwrap();

    // 3. Login as standard_c1 and create a slug
    let public_login_page_url = format!("{}/login", base_url);
    let res = client.get(&public_login_page_url).send().await.unwrap();
    let csrf_token = extract_csrf_token(&res.text().await.unwrap()).unwrap();

    let mut login_params = HashMap::new();
    login_params.insert("username", "standard_c1");
    login_params.insert("password", "password123");
    login_params.insert("csrf_token", &csrf_token);
    client
        .post(&public_login_page_url)
        .form(&login_params)
        .send()
        .await
        .unwrap();

    let res = client
        .get(format!("{}/user/urls", base_url))
        .send()
        .await
        .unwrap();
    let csrf_token = extract_csrf_token(&res.text().await.unwrap()).unwrap();

    let mut create_url_params = HashMap::new();
    create_url_params.insert("destination", "https://yahoo.com");
    create_url_params.insert("code", "");
    create_url_params.insert("custom_slug", "!myyahoo");
    create_url_params.insert("title", "Yahoo");
    create_url_params.insert("description", "Portal");
    create_url_params.insert("tags", "yahoo");
    create_url_params.insert("csrf_token", &csrf_token);
    create_url_params.insert("expires_at", "");
    create_url_params.insert("password", "");
    create_url_params.insert("max_access_count", "");
    create_url_params.insert("utm_source", "");
    create_url_params.insert("utm_medium", "");
    create_url_params.insert("utm_campaign", "");
    client
        .post(format!("{}/user/urls/create", base_url))
        .form(&create_url_params)
        .send()
        .await
        .unwrap();

    // Visit standard_c1's link
    client
        .get(format!("{}/!myyahoo", base_url))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(2500)).await;

    // Verify visit recorded for standard_c1
    let url_uuid = {
        let conn = state.system_db.lock().unwrap();
        conn.query_row(
            "SELECT target_id FROM global_slugs WHERE slug = '!myyahoo';",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
    };

    let res = client
        .get(format!("{}/analytics", base_url))
        .send()
        .await
        .unwrap();
    assert!(res.text().await.unwrap().contains(&url_uuid));

    // Logout standard_c1
    client
        .get(format!("{}/logout", base_url))
        .send()
        .await
        .unwrap();

    // 4. Log in admin to perform the slug transfer to standard_c2
    client
        .get(format!("{}/admin/logout", base_url))
        .send()
        .await
        .unwrap();
    let res = client.get(&login_page_url).send().await.unwrap();
    let csrf_token = extract_csrf_token(&res.text().await.unwrap()).unwrap();
    let mut admin_params = HashMap::new();
    admin_params.insert("username", "admin");
    admin_params.insert("password", "bootstrap-secret");
    admin_params.insert("csrf_token", &csrf_token);
    client
        .post(&login_page_url)
        .form(&admin_params)
        .send()
        .await
        .unwrap();

    // GET /admin/slugs to get CSRF token
    let res = client
        .get(format!("{}/admin/slugs", base_url))
        .send()
        .await
        .unwrap();
    let csrf_token = extract_csrf_token(&res.text().await.unwrap()).unwrap();

    // Transfer slug
    let user2_id_str = user2_id.to_string();
    let mut transfer_params = HashMap::new();
    transfer_params.insert("slug", "!myyahoo");
    transfer_params.insert("new_owner_user_id", &user2_id_str);
    transfer_params.insert("csrf_token", &csrf_token);
    let res = client
        .post(format!("{}/admin/slugs/transfer", base_url))
        .form(&transfer_params)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);

    // 5. Test redirection redirection still works
    let res = client
        .get(format!("{}/!myyahoo", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::MOVED_PERMANENTLY);
    assert_eq!(
        res.headers().get("location").unwrap().to_str().unwrap(),
        "https://yahoo.com"
    );

    // Wait for the background worker to write the standard_c2 visit to the database
    tokio::time::sleep(tokio::time::Duration::from_millis(2500)).await;

    // 6. Verify that visitor logs / analytics for yahoo are now in standard_c2's domain
    client
        .get(format!("{}/admin/logout", base_url))
        .send()
        .await
        .unwrap();

    // Login as standard_c2
    let res = client.get(&public_login_page_url).send().await.unwrap();
    let csrf_token = extract_csrf_token(&res.text().await.unwrap()).unwrap();
    let mut login_c2 = HashMap::new();
    login_c2.insert("username", "standard_c2");
    login_c2.insert("password", "password123");
    login_c2.insert("csrf_token", &csrf_token);
    client
        .post(&public_login_page_url)
        .form(&login_c2)
        .send()
        .await
        .unwrap();

    // standard_c2 analytics should display Yahoo!
    let url_uuid = {
        let conn = state.system_db.lock().unwrap();
        conn.query_row(
            "SELECT target_id FROM global_slugs WHERE slug = '!myyahoo';",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
    };

    let res = client
        .get(format!("{}/analytics", base_url))
        .send()
        .await
        .unwrap();
    assert!(res.text().await.unwrap().contains(&url_uuid));

    let _ = fs::remove_dir_all(&temp_dir);
}
