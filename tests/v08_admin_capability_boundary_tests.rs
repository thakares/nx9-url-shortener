//! v0.8.0 Phase 5 Correction: Admin Core-Only Capability Boundary Tests
//!
//! Required Test Cases:
//! 1. Admin cannot create URL through POST /admin/urls/create (403 Forbidden).
//! 2. Admin cannot create landing page through POST /admin/pages/create (403 Forbidden).
//! 3. Admin cannot create URL through API (403 Forbidden).
//! 4. Admin cannot create landing page through API (403 Forbidden).
//! 5. Admin cannot bulk-create URLs through API (403 Forbidden).
//! 6. Admin GET /admin/urls renders inspection-only registry (no creation form).
//! 7. Admin GET /admin/pages renders inspection-only registry (no creation form).
//! 8. Admin resource inspection still works on global URL registry.
//! 9. Admin resource inspection still works on global page registry.
//! 10. Admin moderation still works.
//! 11. Admin transfer still works.
//! 12. Normal tenant can still create URLs (via UI and API).
//! 13. Normal tenant can still create landing pages (via UI and API).
//! 14. Normal tenant creation uses its own TenantId.
//! 15. No test or creation path uses users/1 as an application tenant.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bzod::analytics::AnalyticsQueue;
use bzod::config::Config;
use bzod::db::topology::Topology;
use bzod::db::Db;
use bzod::state::AppState;
use bzod::web::create_router;
use sha2::{Digest, Sha256};

#[allow(dead_code)]
struct TestHarness {
    temp_dir: PathBuf,
    config: Config,
    db: Db,
    base_url: String,
    admin_client: reqwest::Client,
    user_client: reqwest::Client,
    admin_api_key: String,
    bob_user_id: i64,
    bob_tenant_id: bzod::identity::TenantId,
    bob_token_secret: String,
}

impl TestHarness {
    async fn setup() -> Self {
        let temp_dir =
            std::env::temp_dir().join(format!("bzod_admin_boundary_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let mut config = Config::load();
        config.data_dir = temp_dir.clone();
        config.backup_dir = temp_dir.join("backups");
        config.base_url = Some("http://localhost:8080".to_string());

        let db = Db::init(&config).expect("Db::init failed");
        let (queue, _) = AnalyticsQueue::new(db.clone(), 10, tokio::sync::watch::channel(false).1);
        let state = AppState {
            admin_db: db.admin.clone(),
            system_db: db.system.clone(),
            users_db: db.users.clone(),
            user_dbs: Arc::new(Mutex::new(HashMap::new())),
            db: db.clone(),
            config: config.clone(),
            analytics_queue: queue,
            start_time: Instant::now(),
        };

        let router = create_router(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}", addr);

        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        let admin_client = reqwest::Client::builder()
            .cookie_store(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();

        let user_client = reqwest::Client::builder()
            .cookie_store(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();

        // 1. Create Admin
        let _ = bzod::cli::create_admin::run(
            Some("core_admin".to_string()),
            Some("AdminPass123!".to_string()),
            None,
            config.clone(),
        )
        .await
        .unwrap();

        let admin_user = {
            let conn = db.users.lock().unwrap();
            bzod::db::users::get_user_by_username(&conn, "core_admin")
                .unwrap()
                .expect("Admin must exist")
        };

        // 2. Create Normal Tenant
        let _ = bzod::cli::create_user::run(
            Some("tenant_bob".to_string()),
            Some("UserPass123!".to_string()),
            None,
            config.clone(),
        )
        .await
        .unwrap();

        let (bob_user_id, bob_tenant_id) = {
            let conn = db.users.lock().unwrap();
            let u = bzod::db::users::get_user_by_username(&conn, "tenant_bob")
                .unwrap()
                .expect("Bob must exist");
            (u.id, u.tenant_id.expect("Bob must have a TenantId"))
        };

        // 3. Admin login
        let admin_csrf = extract_csrf(&admin_client, &format!("{}/admin/login", base_url)).await;
        let mut admin_login_params = HashMap::new();
        admin_login_params.insert("username", "core_admin");
        admin_login_params.insert("password", "AdminPass123!");
        admin_login_params.insert("csrf_token", admin_csrf.as_str());

        let admin_login_res = admin_client
            .post(format!("{}/admin/login", base_url))
            .form(&admin_login_params)
            .send()
            .await
            .unwrap();
        assert_eq!(admin_login_res.status(), reqwest::StatusCode::SEE_OTHER);

        // 4. User login
        let user_csrf = extract_csrf(&user_client, &format!("{}/login", base_url)).await;
        let mut user_login_params = HashMap::new();
        user_login_params.insert("username", "tenant_bob");
        user_login_params.insert("password", "UserPass123!");
        user_login_params.insert("csrf_token", user_csrf.as_str());

        let user_login_res = user_client
            .post(format!("{}/login", base_url))
            .form(&user_login_params)
            .send()
            .await
            .unwrap();
        assert_eq!(user_login_res.status(), reqwest::StatusCode::SEE_OTHER);

        // 5. Create API tokens
        let admin_key_secret = format!("bzo_{}", bzod::utils::generate_token(16));
        {
            let mut hasher = Sha256::new();
            hasher.update(admin_key_secret.as_bytes());
            let hashed_key = hex::encode(hasher.finalize());
            let conn = db.admin.lock().unwrap();
            let _ = bzod::db::admin::create_api_key(
                &conn,
                &admin_user.id.to_string(),
                "Admin Test Key",
                &hashed_key,
            )
            .unwrap();
        }

        let bob_token_secret = format!("bzou_{}", bzod::utils::generate_token(16));
        {
            let mut hasher = Sha256::new();
            hasher.update(bob_token_secret.as_bytes());
            let hashed_token = hex::encode(hasher.finalize());
            let conn = db.users.lock().unwrap();
            let _ =
                bzod::db::users::create_user_api_token(&conn, bob_user_id, &hashed_token).unwrap();
        }

        TestHarness {
            temp_dir,
            config,
            db,
            base_url,
            admin_client,
            user_client,
            admin_api_key: admin_key_secret,
            bob_user_id,
            bob_tenant_id,
            bob_token_secret,
        }
    }
}

impl Drop for TestHarness {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.temp_dir);
    }
}

async fn extract_csrf(client: &reqwest::Client, url: &str) -> String {
    let html = client.get(url).send().await.unwrap().text().await.unwrap();
    html.split("name=\"csrf_token\" value=\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
async fn test_01_admin_cannot_create_url_post() {
    let h = TestHarness::setup().await;
    let csrf = extract_csrf(&h.admin_client, &format!("{}/admin/urls", h.base_url)).await;
    let mut form = HashMap::new();
    form.insert("destination", "https://fail.com");
    form.insert("code", "!fail");
    form.insert("csrf_token", csrf.as_str());

    let res = h
        .admin_client
        .post(format!("{}/admin/urls/create", h.base_url))
        .form(&form)
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        reqwest::StatusCode::FORBIDDEN,
        "Admin POST /admin/urls/create must return 403"
    );
}

#[tokio::test]
async fn test_02_admin_cannot_create_landing_page_post() {
    let h = TestHarness::setup().await;
    let csrf = extract_csrf(&h.admin_client, &format!("{}/admin/pages", h.base_url)).await;
    let mut form = HashMap::new();
    form.insert("title", "Admin Page");
    form.insert("slug", "admin-page");
    form.insert("code", "a1b2");
    form.insert("state", "published");
    form.insert("html_content", "<h1>Admin</h1>");
    form.insert("csrf_token", csrf.as_str());

    let res = h
        .admin_client
        .post(format!("{}/admin/pages/create", h.base_url))
        .form(&form)
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        reqwest::StatusCode::FORBIDDEN,
        "Admin POST /admin/pages/create must return 403"
    );
}

#[tokio::test]
async fn test_03_admin_cannot_create_url_api() {
    let h = TestHarness::setup().await;
    let api_client = reqwest::Client::new();
    let res = api_client
        .post(format!("{}/api/v1/urls", h.base_url))
        .header("Authorization", format!("Bearer {}", h.admin_api_key))
        .json(&serde_json::json!({
            "destination": "https://admin-api-fail.com",
            "code": "!admin_api"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        reqwest::StatusCode::FORBIDDEN,
        "Admin API POST /api/v1/urls must return 403"
    );
}

#[tokio::test]
async fn test_04_admin_cannot_create_landing_page_api() {
    let h = TestHarness::setup().await;
    let api_client = reqwest::Client::new();
    let res = api_client
        .post(format!("{}/api/v1/pages", h.base_url))
        .header("Authorization", format!("Bearer {}", h.admin_api_key))
        .json(&serde_json::json!({
            "title": "Admin Page API",
            "slug": "admin-page-api",
            "html_content": "<h1>Admin API</h1>",
            "state": "published"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        reqwest::StatusCode::FORBIDDEN,
        "Admin API POST /api/v1/pages must return 403"
    );
}

#[tokio::test]
async fn test_05_admin_cannot_bulk_create_urls_api() {
    let h = TestHarness::setup().await;
    let api_client = reqwest::Client::new();
    let res = api_client
        .post(format!("{}/api/v1/bulk/url", h.base_url))
        .header("Authorization", format!("Bearer {}", h.admin_api_key))
        .json(&serde_json::json!([
            { "destination": "https://bulk1.com", "code": "!b1" },
            { "destination": "https://bulk2.com", "code": "!b2" }
        ]))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        reqwest::StatusCode::FORBIDDEN,
        "Admin API POST /api/v1/bulk/url must return 403"
    );
}

#[tokio::test]
async fn test_06_admin_urls_registry_renders_no_creation_form() {
    let h = TestHarness::setup().await;
    let html = h
        .admin_client
        .get(format!("{}/admin/urls", h.base_url))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        !html.contains("Shorten a New URL"),
        "Admin /admin/urls must NOT contain 'Shorten a New URL'"
    );
    assert!(
        !html.contains("action=\"/admin/urls/create\""),
        "Admin /admin/urls must NOT contain creation form action"
    );
    assert!(
        html.contains("Global URL Registry"),
        "Admin /admin/urls must render Global URL Registry"
    );
}

#[tokio::test]
async fn test_07_admin_pages_registry_renders_no_creation_form() {
    let h = TestHarness::setup().await;
    let html = h
        .admin_client
        .get(format!("{}/admin/pages", h.base_url))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        !html.contains("Create a New Landing Page"),
        "Admin /admin/pages must NOT contain 'Create a New Landing Page'"
    );
    assert!(
        !html.contains("action=\"/admin/pages/create\""),
        "Admin /admin/pages must NOT contain creation form action"
    );
    assert!(
        html.contains("Global Landing Page Registry"),
        "Admin /admin/pages must render Global Landing Page Registry"
    );
}

#[tokio::test]
async fn test_08_and_09_admin_inspection_of_global_registries() {
    let h = TestHarness::setup().await;
    let api_client = reqwest::Client::new();

    // Bob creates URL
    let res = api_client
        .post(format!("{}/api/v1/urls", h.base_url))
        .header("Authorization", format!("Bearer {}", h.bob_token_secret))
        .json(&serde_json::json!({
            "destination": "https://bob-portfolio.org",
            "code": "b0b001"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::CREATED);

    // Bob creates Landing Page via UI
    let bob_pages_csrf = extract_csrf(&h.user_client, &format!("{}/user/pages", h.base_url)).await;
    let mut bob_page_form = HashMap::new();
    bob_page_form.insert("title", "Bob Landing Page");
    bob_page_form.insert("slug", "bob-page");
    bob_page_form.insert("code", "a1b2");
    bob_page_form.insert("custom_slug", "");
    bob_page_form.insert("state", "published");
    bob_page_form.insert("html_content", "<h1>Welcome to Bob's Page</h1>");
    bob_page_form.insert("csrf_token", bob_pages_csrf.as_str());

    let res = h
        .user_client
        .post(format!("{}/user/pages/create", h.base_url))
        .form(&bob_page_form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);

    // Test 8: Admin can inspect URL registry
    let admin_urls_inspected = h
        .admin_client
        .get(format!("{}/admin/urls", h.base_url))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        admin_urls_inspected.contains("b0b001"),
        "Admin URL registry must display tenant URLs"
    );
    assert!(
        admin_urls_inspected.contains("tenant_bob"),
        "Admin URL registry must display owner username"
    );
    assert!(
        admin_urls_inspected.contains(h.bob_tenant_id.as_str()),
        "Admin URL registry must display owner TenantId"
    );

    // Test 9: Admin can inspect Landing Page registry
    let admin_pages_inspected = h
        .admin_client
        .get(format!("{}/admin/pages", h.base_url))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        admin_pages_inspected.contains("Bob Landing Page"),
        "Admin Page registry must display tenant landing pages"
    );
    assert!(
        admin_pages_inspected.contains("tenant_bob"),
        "Admin Page registry must display owner username"
    );
    assert!(
        admin_pages_inspected.contains(h.bob_tenant_id.as_str()),
        "Admin Page registry must display owner TenantId"
    );
}

#[tokio::test]
async fn test_10_admin_moderation_functional() {
    let h = TestHarness::setup().await;
    let api_client = reqwest::Client::new();

    let _ = api_client
        .post(format!("{}/api/v1/urls", h.base_url))
        .header("Authorization", format!("Bearer {}", h.bob_token_secret))
        .json(&serde_json::json!({
            "destination": "https://spam.org",
            "code": "!spam-link"
        }))
        .send()
        .await
        .unwrap();

    let urls_conn = h.db.global_urls.lock().unwrap();
    let pages_conn = h.db.global_landing_pages.lock().unwrap();
    let retired = bzod::db::slugs::retire_slug(&urls_conn, &pages_conn, "!spam-link").unwrap();
    assert!(retired, "Admin moderation (retire slug) must succeed");
}

#[tokio::test]
async fn test_11_admin_transfer_functional() {
    let h = TestHarness::setup().await;
    let api_client = reqwest::Client::new();

    let _ = api_client
        .post(format!("{}/api/v1/urls", h.base_url))
        .header("Authorization", format!("Bearer {}", h.bob_token_secret))
        .json(&serde_json::json!({
            "destination": "https://transfer-target.org",
            "code": "!transfer-link"
        }))
        .send()
        .await
        .unwrap();

    let _ = bzod::cli::create_user::run(
        Some("tenant_alice".to_string()),
        Some("AlicePass123!".to_string()),
        None,
        h.config.clone(),
    )
    .await
    .unwrap();

    let alice_tenant_id = {
        let conn = h.db.users.lock().unwrap();
        let u = bzod::db::users::get_user_by_username(&conn, "tenant_alice")
            .unwrap()
            .expect("Alice must exist");
        u.tenant_id.expect("Alice must have a TenantId")
    };

    let urls_conn = h.db.global_urls.lock().unwrap();
    let pages_conn = h.db.global_landing_pages.lock().unwrap();
    let transferred = bzod::db::slugs::transfer_slug_owner(
        &urls_conn,
        &pages_conn,
        "!transfer-link",
        &alice_tenant_id,
        "new_target_id",
    )
    .unwrap();
    assert!(transferred, "Admin slug transfer to Alice must succeed");

    let lookup = bzod::db::slugs::lookup_slug(&urls_conn, &pages_conn, "!transfer-link")
        .unwrap()
        .unwrap();
    assert_eq!(
        lookup.owner_tenant_id,
        alice_tenant_id.as_str(),
        "Slug owner must now be Alice's TenantId"
    );
}

#[tokio::test]
async fn test_12_13_14_15_normal_tenant_creation_and_topology() {
    let h = TestHarness::setup().await;

    // Verify User UI contains creation form
    let user_urls_html = h
        .user_client
        .get(format!("{}/user/urls", h.base_url))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        user_urls_html.contains("Create a New URL"),
        "User /user/urls MUST contain creation form"
    );
    assert!(
        user_urls_html.contains("action=\"/user/urls/create\""),
        "User /user/urls MUST post to /user/urls/create"
    );

    // UI URL creation
    let bob_urls_csrf = extract_csrf(&h.user_client, &format!("{}/user/urls", h.base_url)).await;
    let mut bob_url_form = HashMap::new();
    bob_url_form.insert("destination", "https://bob-portfolio.org");
    bob_url_form.insert("code", "b0b001");
    bob_url_form.insert("title", "Bob's Portfolio");
    bob_url_form.insert("csrf_token", bob_urls_csrf.as_str());

    let res = h
        .user_client
        .post(format!("{}/user/urls/create", h.base_url))
        .form(&bob_url_form)
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        reqwest::StatusCode::SEE_OTHER,
        "User URL creation must succeed and redirect"
    );

    // UI Landing page creation
    let bob_pages_csrf = extract_csrf(&h.user_client, &format!("{}/user/pages", h.base_url)).await;
    let mut bob_page_form = HashMap::new();
    bob_page_form.insert("title", "Bob Landing Page");
    bob_page_form.insert("slug", "bob-page");
    bob_page_form.insert("code", "a1b2");
    bob_page_form.insert("custom_slug", "");
    bob_page_form.insert("state", "published");
    bob_page_form.insert("html_content", "<h1>Welcome to Bob's Page</h1>");
    bob_page_form.insert("csrf_token", bob_pages_csrf.as_str());

    let res = h
        .user_client
        .post(format!("{}/user/pages/create", h.base_url))
        .form(&bob_page_form)
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        reqwest::StatusCode::SEE_OTHER,
        "User landing page creation must succeed and redirect"
    );

    // Test 14: Verify content is stored under users/<TenantId>/content.db
    let topology = Topology::new(&h.temp_dir);
    let bob_tenant_dir = topology.user_dir(h.bob_tenant_id.as_str()).unwrap();
    assert!(
        bob_tenant_dir.join("content.db").exists(),
        "Bob's content.db must exist"
    );

    let bob_conn = rusqlite::Connection::open(bob_tenant_dir.join("content.db")).unwrap();
    let url_count: i64 = bob_conn
        .query_row("SELECT COUNT(*) FROM urls;", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        url_count, 1,
        "Bob's content.db must hold exactly 1 URL created by Bob"
    );

    let page_count: i64 = bob_conn
        .query_row("SELECT COUNT(*) FROM landing_pages;", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        page_count, 1,
        "Bob's content.db must hold exactly 1 landing page created by Bob"
    );

    // Test 15: No creation path uses users/1 or integer paths
    let legacy_int_dir = h.temp_dir.join("users").join("1");
    assert!(!legacy_int_dir.exists(), "users/1 must NOT exist anywhere");
}
