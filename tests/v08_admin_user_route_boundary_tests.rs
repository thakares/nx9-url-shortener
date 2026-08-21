//! v0.8.0 Phase 5-Correction-2: Strict Admin/Core vs Normal Tenant Route Boundary Tests
//!
//! Required Test Cases:
//! 1. admin_login_redirects_to_admin_dashboard
//! 2. admin_cannot_access_user_dashboard
//! 3. admin_cannot_access_user_urls
//! 4. admin_cannot_access_user_pages
//! 5. admin_cannot_access_user_settings
//! 6. admin_cannot_access_user_audit
//! 7. admin_cannot_create_url_via_user_route
//! 8. admin_cannot_create_page_via_user_route
//! 9. normal_user_can_access_user_dashboard
//! 10. normal_user_can_access_user_urls
//! 11. normal_user_can_access_user_pages
//! 12. normal_user_can_access_user_settings
//! 13. normal_user_can_create_url
//! 14. normal_user_can_create_page
//! 15. normal_user_cannot_access_admin_routes
//! 16. admin_has_no_tenant_database
//! 17. admin_has_no_tenant_directory
//! 18. no_users_1_tenant_fallback
//! 19. tenant_routes_require_tenant_id

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bzod::analytics::AnalyticsQueue;
use bzod::config::Config;
use bzod::db::topology::Topology;
use bzod::db::Db;
use bzod::identity::TenantId;
use bzod::state::AppState;
use bzod::web::create_router;

#[allow(dead_code)]
struct TestHarness {
    temp_dir: PathBuf,
    config: Config,
    db: Db,
    base_url: String,
    admin_client: reqwest::Client,
    user_client: reqwest::Client,
    admin_username: String,
    admin_user_id: i64,
    bob_username: String,
    bob_user_id: i64,
    bob_tenant_id: TenantId,
}

impl TestHarness {
    async fn setup() -> Self {
        let temp_dir =
            std::env::temp_dir().join(format!("bzod_user_boundary_{}", uuid::Uuid::new_v4()));
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

        // 2. Create Normal Tenant User (Bob)
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

        // 3. Admin login via /admin/login
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

        // 4. Normal user login via /login
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

        TestHarness {
            temp_dir,
            config,
            db,
            base_url,
            admin_client,
            user_client,
            admin_username: "core_admin".to_string(),
            admin_user_id: admin_user.id,
            bob_username: "tenant_bob".to_string(),
            bob_user_id,
            bob_tenant_id,
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

// ---------------------------------------------------------------------------
// 1. Admin login always lands on /admin/dashboard (both via /admin/login and /login)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_01_admin_login_redirects_to_admin_dashboard() {
    let h = TestHarness::setup().await;
    let fresh_client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    // Login via /login as Admin
    let csrf = extract_csrf(&fresh_client, &format!("{}/login", h.base_url)).await;
    let mut form = HashMap::new();
    form.insert("username", "core_admin");
    form.insert("password", "AdminPass123!");
    form.insert("csrf_token", csrf.as_str());

    let res = fresh_client
        .post(format!("{}/login", h.base_url))
        .form(&form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);
    let location = res.headers().get("Location").unwrap().to_str().unwrap();
    assert_eq!(
        location, "/admin/dashboard",
        "Admin logging in via /login MUST redirect to /admin/dashboard"
    );
}

// ---------------------------------------------------------------------------
// 2. Admin cannot access /user/dashboard (403 Forbidden)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_02_admin_cannot_access_user_dashboard() {
    let h = TestHarness::setup().await;
    let res = h
        .admin_client
        .get(format!("{}/user/dashboard", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        reqwest::StatusCode::FORBIDDEN,
        "Admin accessing /user/dashboard MUST return 403 Forbidden"
    );
}

// ---------------------------------------------------------------------------
// 3. Admin cannot access /user/urls (403 Forbidden)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_03_admin_cannot_access_user_urls() {
    let h = TestHarness::setup().await;
    let res = h
        .admin_client
        .get(format!("{}/user/urls", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        reqwest::StatusCode::FORBIDDEN,
        "Admin accessing /user/urls MUST return 403 Forbidden"
    );
}

// ---------------------------------------------------------------------------
// 4. Admin cannot access /user/pages (403 Forbidden)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_04_admin_cannot_access_user_pages() {
    let h = TestHarness::setup().await;
    let res = h
        .admin_client
        .get(format!("{}/user/pages", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        reqwest::StatusCode::FORBIDDEN,
        "Admin accessing /user/pages MUST return 403 Forbidden"
    );
}

// ---------------------------------------------------------------------------
// 5. Admin cannot access /user/settings (403 Forbidden)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_05_admin_cannot_access_user_settings() {
    let h = TestHarness::setup().await;
    let res = h
        .admin_client
        .get(format!("{}/user/settings", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        reqwest::StatusCode::FORBIDDEN,
        "Admin accessing /user/settings MUST return 403 Forbidden"
    );
}

// ---------------------------------------------------------------------------
// 6. Admin cannot access /user/audit (403 Forbidden)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_06_admin_cannot_access_user_audit() {
    let h = TestHarness::setup().await;
    let res = h
        .admin_client
        .get(format!("{}/user/audit", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        reqwest::StatusCode::FORBIDDEN,
        "Admin accessing /user/audit MUST return 403 Forbidden"
    );
}

// ---------------------------------------------------------------------------
// 7. Admin cannot create URL via user route POST /user/urls/create (403 Forbidden, 0 writes)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_07_admin_cannot_create_url_via_user_route() {
    let h = TestHarness::setup().await;
    let mut form = HashMap::new();
    form.insert("destination", "https://admin-attack.org");
    form.insert("code", "a1b2c3");
    form.insert("csrf_token", "invalid_or_any");

    let res = h
        .admin_client
        .post(format!("{}/user/urls/create", h.base_url))
        .form(&form)
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        reqwest::StatusCode::FORBIDDEN,
        "Admin POST /user/urls/create MUST return 403 Forbidden"
    );

    // Verify 0 writes in global slug registry
    let urls_conn = h.db.global_urls.lock().unwrap();
    let count: i64 = urls_conn
        .query_row(
            "SELECT COUNT(*) FROM global_urls WHERE slug = 'a1b2c3';",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        count, 0,
        "No global slug record may be created for rejected admin request"
    );
}

// ---------------------------------------------------------------------------
// 8. Admin cannot create landing page via user route POST /user/pages/create (403 Forbidden, 0 writes)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_08_admin_cannot_create_page_via_user_route() {
    let h = TestHarness::setup().await;
    let mut form = HashMap::new();
    form.insert("title", "Admin Page");
    form.insert("slug", "admin-page-attack");
    form.insert("code", "c1d2");
    form.insert("custom_slug", "");
    form.insert("state", "published");
    form.insert("html_content", "<h1>Attack</h1>");
    form.insert("csrf_token", "invalid_or_any");

    let res = h
        .admin_client
        .post(format!("{}/user/pages/create", h.base_url))
        .form(&form)
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        reqwest::StatusCode::FORBIDDEN,
        "Admin POST /user/pages/create MUST return 403 Forbidden"
    );

    // Verify 0 writes in global slug registry
    let pages_conn = h.db.global_landing_pages.lock().unwrap();
    let count: i64 = pages_conn
        .query_row(
            "SELECT COUNT(*) FROM global_landing_pages WHERE slug = 'c1d2';",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        count, 0,
        "No global landing page record may be created for rejected admin request"
    );
}

// ---------------------------------------------------------------------------
// 9. Normal user can access /user/dashboard (200 OK)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_09_normal_user_can_access_user_dashboard() {
    let h = TestHarness::setup().await;
    let res = h
        .user_client
        .get(format!("{}/user/dashboard", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        reqwest::StatusCode::OK,
        "Normal user MUST be able to access /user/dashboard"
    );
}

// ---------------------------------------------------------------------------
// 10. Normal user can access /user/urls (200 OK)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_10_normal_user_can_access_user_urls() {
    let h = TestHarness::setup().await;
    let res = h
        .user_client
        .get(format!("{}/user/urls", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        reqwest::StatusCode::OK,
        "Normal user MUST be able to access /user/urls"
    );
    let html = res.text().await.unwrap();
    assert!(
        html.contains("Create a New URL"),
        "Normal user /user/urls MUST contain creation form"
    );
}

// ---------------------------------------------------------------------------
// 11. Normal user can access /user/pages (200 OK)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_11_normal_user_can_access_user_pages() {
    let h = TestHarness::setup().await;
    let res = h
        .user_client
        .get(format!("{}/user/pages", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        reqwest::StatusCode::OK,
        "Normal user MUST be able to access /user/pages"
    );
    let html = res.text().await.unwrap();
    assert!(
        html.contains("Create a New Landing Page"),
        "Normal user /user/pages MUST contain creation form"
    );
}

// ---------------------------------------------------------------------------
// 12. Normal user can access /user/settings (200 OK)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_12_normal_user_can_access_user_settings() {
    let h = TestHarness::setup().await;
    let res = h
        .user_client
        .get(format!("{}/user/settings", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        reqwest::StatusCode::OK,
        "Normal user MUST be able to access /user/settings"
    );
}

// ---------------------------------------------------------------------------
// 13. Normal user can create URL via POST /user/urls/create (303 Redirect)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_13_normal_user_can_create_url() {
    let h = TestHarness::setup().await;
    let csrf = extract_csrf(&h.user_client, &format!("{}/user/urls", h.base_url)).await;
    let mut form = HashMap::new();
    form.insert("destination", "https://bob-portfolio.org");
    form.insert("code", "b0b001");
    form.insert("title", "Bob Portfolio");
    form.insert("csrf_token", csrf.as_str());

    let res = h
        .user_client
        .post(format!("{}/user/urls/create", h.base_url))
        .form(&form)
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        reqwest::StatusCode::SEE_OTHER,
        "User URL creation must succeed and redirect"
    );

    // Verify written to tenant content.db
    let topology = Topology::new(&h.temp_dir);
    let bob_dir = topology.user_dir(h.bob_tenant_id.as_str()).unwrap();
    let conn = rusqlite::Connection::open(bob_dir.join("content.db")).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM urls WHERE code = 'b0b001';",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "URL must be stored in Bob's content.db");

    // Verify written to global_urls.db
    let urls_conn = h.db.global_urls.lock().unwrap();
    let owner_tid: String = urls_conn
        .query_row(
            "SELECT owner_tenant_id FROM global_urls WHERE slug = 'b0b001';",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        owner_tid,
        h.bob_tenant_id.as_str(),
        "Global slug owner must be Bob's TenantId"
    );
}

// ---------------------------------------------------------------------------
// 14. Normal user can create landing page via POST /user/pages/create (303 Redirect)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_14_normal_user_can_create_page() {
    let h = TestHarness::setup().await;
    let csrf = extract_csrf(&h.user_client, &format!("{}/user/pages", h.base_url)).await;
    let mut form = HashMap::new();
    form.insert("title", "Bob Landing Page");
    form.insert("slug", "bob-landing");
    form.insert("code", "a1b2");
    form.insert("custom_slug", "");
    form.insert("state", "published");
    form.insert("html_content", "<h1>Welcome</h1>");
    form.insert("csrf_token", csrf.as_str());

    let res = h
        .user_client
        .post(format!("{}/user/pages/create", h.base_url))
        .form(&form)
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        reqwest::StatusCode::SEE_OTHER,
        "User landing page creation must succeed and redirect"
    );

    // Verify written to tenant content.db
    let topology = Topology::new(&h.temp_dir);
    let bob_dir = topology.user_dir(h.bob_tenant_id.as_str()).unwrap();
    let conn = rusqlite::Connection::open(bob_dir.join("content.db")).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM landing_pages WHERE slug = 'bob-landing';",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "Page must be stored in Bob's content.db");
}

// ---------------------------------------------------------------------------
// 15. Normal user cannot access /admin/* routes (403 Forbidden)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_15_normal_user_cannot_access_admin_routes() {
    let h = TestHarness::setup().await;

    let admin_paths = vec![
        "/admin/dashboard",
        "/admin/urls",
        "/admin/pages",
        "/admin/users",
        "/admin/settings",
        "/admin/audit",
        "/admin/status",
    ];

    for path in admin_paths {
        let res = h
            .user_client
            .get(format!("{}{}", h.base_url, path))
            .send()
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            reqwest::StatusCode::FORBIDDEN,
            "Normal user accessing {} MUST be rejected with 403 Forbidden",
            path
        );
    }
}

// ---------------------------------------------------------------------------
// 16. Admin has no tenant database
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_16_admin_has_no_tenant_database() {
    let h = TestHarness::setup().await;
    // Core Admin has no TenantId in users.db
    let conn = h.db.users.lock().unwrap();
    let admin_u = bzod::db::users::get_user_by_username(&conn, "core_admin")
        .unwrap()
        .unwrap();
    assert!(
        admin_u.tenant_id.is_none(),
        "Core Admin must have tenant_id == None"
    );
}

// ---------------------------------------------------------------------------
// 17. Admin has no tenant directory
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_17_admin_has_no_tenant_directory() {
    let h = TestHarness::setup().await;
    let users_dir = h.temp_dir.join("users");
    if users_dir.exists() {
        for entry in fs::read_dir(users_dir).unwrap().flatten() {
            let file_name = entry.file_name().to_string_lossy().to_string();
            assert_ne!(file_name, "admin", "No tenant directory 'admin' may exist");
            assert_ne!(
                file_name, "core_admin",
                "No tenant directory 'core_admin' may exist"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 18. No users/1 fallback
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_18_no_users_1_tenant_fallback() {
    let h = TestHarness::setup().await;
    let legacy_dir = h.temp_dir.join("users").join("1");
    assert!(
        !legacy_dir.exists(),
        "users/1 directory must NEVER be created or used as fallback"
    );
}

// ---------------------------------------------------------------------------
// 19. Tenant routes require TenantId (unprovisioned user without TenantId is rejected)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_19_tenant_routes_require_tenant_id() {
    let h = TestHarness::setup().await;

    // Insert user with status active, account_type user, but tenant_id NULL
    {
        let conn = h.db.users.lock().unwrap();
        let hash = bzod::auth::hash_password("Pass123!").unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO users (username, password_hash, status, created_at, account_type, tenant_id, uuid) 
             VALUES ('unprovisioned_user', ?1, 'active', ?2, 'user', NULL, NULL);",
            rusqlite::params![hash, now],
        )
        .unwrap();
    }

    let unpriv_client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let csrf = extract_csrf(&unpriv_client, &format!("{}/login", h.base_url)).await;
    let mut form = HashMap::new();
    form.insert("username", "unprovisioned_user");
    form.insert("password", "Pass123!");
    form.insert("csrf_token", csrf.as_str());

    let res = unpriv_client
        .post(format!("{}/login", h.base_url))
        .form(&form)
        .send()
        .await
        .unwrap();

    // Login must fail or redirect with error because tenant_id is missing
    let location = res
        .headers()
        .get("Location")
        .map(|v| v.to_str().unwrap())
        .unwrap_or_default();
    assert!(
        location.contains("error=Invalid+tenant+configuration") || location.contains("error="),
        "Unprovisioned user without TenantId must NOT be logged in as a valid tenant"
    );
}

// ---------------------------------------------------------------------------
// 20. Admin login clears existing user session & cookie
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_20_admin_login_clears_existing_user_session() {
    let h = TestHarness::setup().await;
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    // 1. Login as user first
    let user_csrf = extract_csrf(&client, &format!("{}/login", h.base_url)).await;
    let mut user_form = HashMap::new();
    user_form.insert("username", "tenant_bob");
    user_form.insert("password", "UserPass123!");
    user_form.insert("csrf_token", user_csrf.as_str());

    let res = client
        .post(format!("{}/login", h.base_url))
        .form(&user_form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);

    // Verify user can access dashboard
    let res = client
        .get(format!("{}/user/dashboard", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);

    // 2. Now login as Admin on /admin/login
    let admin_csrf = extract_csrf(&client, &format!("{}/admin/login", h.base_url)).await;
    let mut admin_form = HashMap::new();
    admin_form.insert("username", "core_admin");
    admin_form.insert("password", "AdminPass123!");
    admin_form.insert("csrf_token", admin_csrf.as_str());

    let res = client
        .post(format!("{}/admin/login", h.base_url))
        .form(&admin_form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);
    assert_eq!(res.headers().get("location").unwrap(), "/admin/dashboard");

    // Verify Admin can access admin dashboard
    let res = client
        .get(format!("{}/admin/dashboard", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);

    // Verify Admin is now blocked from user dashboard (403 Forbidden) and has no active user session
    let res = client
        .get(format!("{}/user/dashboard", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// 21. User login clears existing admin session & cookie
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_21_user_login_clears_existing_admin_session() {
    let h = TestHarness::setup().await;
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    // 1. Login as Admin first
    let admin_csrf = extract_csrf(&client, &format!("{}/admin/login", h.base_url)).await;
    let mut admin_form = HashMap::new();
    admin_form.insert("username", "core_admin");
    admin_form.insert("password", "AdminPass123!");
    admin_form.insert("csrf_token", admin_csrf.as_str());

    let res = client
        .post(format!("{}/admin/login", h.base_url))
        .form(&admin_form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);

    // Verify admin can access admin dashboard
    let res = client
        .get(format!("{}/admin/dashboard", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);

    // 2. Now login as user on /login
    let user_csrf = extract_csrf(&client, &format!("{}/login", h.base_url)).await;
    let mut user_form = HashMap::new();
    user_form.insert("username", "tenant_bob");
    user_form.insert("password", "UserPass123!");
    user_form.insert("csrf_token", user_csrf.as_str());

    let res = client
        .post(format!("{}/login", h.base_url))
        .form(&user_form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);
    assert_eq!(res.headers().get("location").unwrap(), "/user/dashboard");

    // Verify user can access user dashboard
    let res = client
        .get(format!("{}/user/dashboard", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);

    // Verify user is blocked from admin dashboard (403 Forbidden) and has no active admin session
    let res = client
        .get(format!("{}/admin/dashboard", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// 22. Admin logout invalidates admin session in DB and browser
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_22_admin_logout_invalidates_admin_session() {
    let h = TestHarness::setup().await;
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    // Login as Admin
    let admin_csrf = extract_csrf(&client, &format!("{}/admin/login", h.base_url)).await;
    let mut admin_form = HashMap::new();
    admin_form.insert("username", "core_admin");
    admin_form.insert("password", "AdminPass123!");
    admin_form.insert("csrf_token", admin_csrf.as_str());

    let res = client
        .post(format!("{}/admin/login", h.base_url))
        .form(&admin_form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);

    // Logout via /admin/logout
    let res = client
        .get(format!("{}/admin/logout", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);
    assert_eq!(res.headers().get("location").unwrap(), "/admin/login");

    // Revisit /admin/dashboard -> Must redirect to /admin/login
    let res = client
        .get(format!("{}/admin/dashboard", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);
    assert_eq!(res.headers().get("location").unwrap(), "/admin/login");
}

// ---------------------------------------------------------------------------
// 23. User logout invalidates user session in DB and browser
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_23_user_logout_invalidates_user_session() {
    let h = TestHarness::setup().await;
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    // Login as user
    let user_csrf = extract_csrf(&client, &format!("{}/login", h.base_url)).await;
    let mut user_form = HashMap::new();
    user_form.insert("username", "tenant_bob");
    user_form.insert("password", "UserPass123!");
    user_form.insert("csrf_token", user_csrf.as_str());

    let res = client
        .post(format!("{}/login", h.base_url))
        .form(&user_form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);

    // Logout via /logout
    let res = client
        .get(format!("{}/logout", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);
    assert_eq!(res.headers().get("location").unwrap(), "/login");

    // Revisit /user/dashboard -> Must redirect to /login
    let res = client
        .get(format!("{}/user/dashboard", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);
    assert_eq!(res.headers().get("location").unwrap(), "/login");
}

// ---------------------------------------------------------------------------
// 24. Fresh browser requires admin credentials
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_24_fresh_browser_requires_admin_credentials() {
    let h = TestHarness::setup().await;
    let fresh_client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let res = fresh_client
        .get(format!("{}/admin/dashboard", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);
    assert_eq!(res.headers().get("location").unwrap(), "/admin/login");
}

// ---------------------------------------------------------------------------
// 25. Fresh browser requires user credentials
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_25_fresh_browser_requires_user_credentials() {
    let h = TestHarness::setup().await;
    let fresh_client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let res = fresh_client
        .get(format!("{}/user/dashboard", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);
    assert_eq!(res.headers().get("location").unwrap(), "/login");
}

// ---------------------------------------------------------------------------
// 26. Admin to User login switch
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_26_admin_to_user_login_switch() {
    let h = TestHarness::setup().await;
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    // 1. Admin login
    let admin_csrf = extract_csrf(&client, &format!("{}/admin/login", h.base_url)).await;
    let mut admin_form = HashMap::new();
    admin_form.insert("username", "core_admin");
    admin_form.insert("password", "AdminPass123!");
    admin_form.insert("csrf_token", admin_csrf.as_str());

    let res = client
        .post(format!("{}/admin/login", h.base_url))
        .form(&admin_form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);

    // Verify /admin/dashboard accessible
    let res = client
        .get(format!("{}/admin/dashboard", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);

    // Verify /user/dashboard forbidden
    let res = client
        .get(format!("{}/user/dashboard", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::FORBIDDEN);

    // 2. Switch to User by logging in at /login
    let user_csrf = extract_csrf(&client, &format!("{}/login", h.base_url)).await;
    let mut user_form = HashMap::new();
    user_form.insert("username", "tenant_bob");
    user_form.insert("password", "UserPass123!");
    user_form.insert("csrf_token", user_csrf.as_str());

    let res = client
        .post(format!("{}/login", h.base_url))
        .form(&user_form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);
    assert_eq!(res.headers().get("location").unwrap(), "/user/dashboard");

    // Verify /user/dashboard now accessible (200 OK)
    let res = client
        .get(format!("{}/user/dashboard", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);

    // Verify User can create URL
    let url_csrf = extract_csrf(&client, &format!("{}/user/urls", h.base_url)).await;
    let mut url_form = HashMap::new();
    url_form.insert("destination", "https://bob-switch.org");
    url_form.insert("code", "b0bsw1");
    url_form.insert("title", "Bob Switch");
    url_form.insert("csrf_token", url_csrf.as_str());

    let res = client
        .post(format!("{}/user/urls/create", h.base_url))
        .form(&url_form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);

    // Verify /admin/dashboard is now forbidden (403)
    let res = client
        .get(format!("{}/admin/dashboard", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// 27. User to Admin login switch
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_27_user_to_admin_login_switch() {
    let h = TestHarness::setup().await;
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    // 1. User login
    let user_csrf = extract_csrf(&client, &format!("{}/login", h.base_url)).await;
    let mut user_form = HashMap::new();
    user_form.insert("username", "tenant_bob");
    user_form.insert("password", "UserPass123!");
    user_form.insert("csrf_token", user_csrf.as_str());

    let res = client
        .post(format!("{}/login", h.base_url))
        .form(&user_form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);

    // Verify /user/dashboard accessible
    let res = client
        .get(format!("{}/user/dashboard", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);

    // Verify /admin/dashboard forbidden
    let res = client
        .get(format!("{}/admin/dashboard", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::FORBIDDEN);

    // 2. Switch to Admin by logging in at /admin/login
    let admin_csrf = extract_csrf(&client, &format!("{}/admin/login", h.base_url)).await;
    let mut admin_form = HashMap::new();
    admin_form.insert("username", "core_admin");
    admin_form.insert("password", "AdminPass123!");
    admin_form.insert("csrf_token", admin_csrf.as_str());

    let res = client
        .post(format!("{}/admin/login", h.base_url))
        .form(&admin_form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);
    assert_eq!(res.headers().get("location").unwrap(), "/admin/dashboard");

    // Verify /admin/dashboard now accessible (200 OK)
    let res = client
        .get(format!("{}/admin/dashboard", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);

    // Verify /user/dashboard is now forbidden (403)
    let res = client
        .get(format!("{}/user/dashboard", h.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::FORBIDDEN);
}
