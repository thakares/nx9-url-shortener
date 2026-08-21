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

fn create_temp_config(temp_dir: PathBuf) -> Config {
    let mut config = Config::load();
    config.data_dir = temp_dir.clone();
    config.backup_dir = temp_dir.join("backups");
    config.base_url = Some("http://localhost:8080".to_string());
    config
}

fn build_test_state(db: &Db, config: &Config) -> AppState {
    let (queue, _) = AnalyticsQueue::new(db.clone(), 10, tokio::sync::watch::channel(false).1);
    AppState {
        admin_db: db.admin.clone(),
        system_db: db.system.clone(),
        users_db: db.users.clone(),
        user_dbs: Arc::new(Mutex::new(HashMap::new())),
        db: db.clone(),
        config: config.clone(),
        analytics_queue: queue,
        start_time: Instant::now(),
    }
}

#[tokio::test]
async fn test_admin_provisioning_has_no_tenant_directory() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_p5_admin_dir_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    // 1. Initialize Db
    let db = Db::init(&config).expect("Db::init failed");
    let topology = Topology::new(&temp_dir);

    // 2. Create Admin user via CLI
    let admin_res = bzod::cli::create_admin::run(
        Some("platform_admin".to_string()),
        Some("supersecretpass123".to_string()),
        None,
        config.clone(),
    )
    .await;
    assert!(admin_res.is_ok());

    // 3. Verify Admin in admin.db and users.db
    let admin_user = {
        let users_conn = db.users.lock().unwrap();
        let u = bzod::db::users::get_user_by_username(&users_conn, "platform_admin")
            .unwrap()
            .expect("Admin user must exist in users.db");
        assert_eq!(u.account_type, "admin");
        assert!(u.tenant_id.is_none(), "Admin must NOT have a TenantId");
        u
    };

    // 4. Verify NO tenant directory was created for admin
    let users_dir = topology.users_dir();
    if users_dir.exists() {
        let entries: Vec<_> = fs::read_dir(&users_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        for entry in entries {
            let name = entry.file_name().to_string_lossy().to_string();
            assert_ne!(name, format!("{}", admin_user.id));
            assert_ne!(name, "platform_admin");
        }
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_admin_cannot_create_unowned_application_resources() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_p5_unowned_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Db::init failed");
    let state = build_test_state(&db, &config);

    let router = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    // 1. Create Admin
    let _ = bzod::cli::create_admin::run(
        Some("operator".to_string()),
        Some("pass123456".to_string()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    // 2. Log in as admin
    let login_page = client
        .get(format!("{}/admin/login", base_url))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    // Extract CSRF token
    let csrf = login_page
        .split("name=\"csrf_token\" value=\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .unwrap_or_default();

    let mut params = HashMap::new();
    params.insert("username", "operator");
    params.insert("password", "pass123456");
    params.insert("csrf_token", csrf);

    let login_res = client
        .post(format!("{}/admin/login", base_url))
        .form(&params)
        .send()
        .await
        .unwrap();
    assert_eq!(login_res.status(), reqwest::StatusCode::SEE_OTHER);

    // 3. Admin attempts to create URL via POST /admin/urls/create -> Should redirect with error
    let mut url_form = HashMap::new();
    url_form.insert("destination", "https://example.com");
    url_form.insert("code", "!admin_link");
    url_form.insert("csrf_token", csrf);

    let create_url_res = client
        .post(format!("{}/admin/urls/create", base_url))
        .form(&url_form)
        .send()
        .await
        .unwrap();
    assert_eq!(create_url_res.status(), reqwest::StatusCode::FORBIDDEN);

    // 4. Admin attempts to create Landing Page via POST /admin/pages/create -> Should return FORBIDDEN
    let mut page_form = HashMap::new();
    page_form.insert("title", "Admin Page");
    page_form.insert("slug", "!admin_page");
    page_form.insert("code", "!admin_page");
    page_form.insert("custom_slug", "");
    page_form.insert("state", "published");
    page_form.insert("html_content", "<h1>Admin</h1>");
    page_form.insert("csrf_token", csrf);

    let create_page_res = client
        .post(format!("{}/admin/pages/create", base_url))
        .form(&page_form)
        .send()
        .await
        .unwrap();
    assert_eq!(create_page_res.status(), reqwest::StatusCode::FORBIDDEN);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_platform_click_aggregation_without_admin_analytics() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_p5_agg_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Db::init failed");

    // 1. Create two standard tenant users
    let _ = bzod::cli::create_user::run(
        Some("tenant_one".to_string()),
        Some("pass123".to_string()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    let _ = bzod::cli::create_user::run(
        Some("tenant_two".to_string()),
        Some("pass123".to_string()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    let (u1, u2) = {
        let conn = db.users.lock().unwrap();
        let user1 = bzod::db::users::get_user_by_username(&conn, "tenant_one")
            .unwrap()
            .unwrap();
        let user2 = bzod::db::users::get_user_by_username(&conn, "tenant_two")
            .unwrap()
            .unwrap();
        (user1, user2)
    };

    // 2. Insert clicks directly into tenant 1 and tenant 2 analytics DBs
    let t1_tid = u1.tenant_id.unwrap();
    let t2_tid = u2.tenant_id.unwrap();

    let t1_analytics_path = db.topology.tenant_analytics_db(t1_tid);
    let t2_analytics_path = db.topology.tenant_analytics_db(t2_tid);

    {
        let conn1 = rusqlite::Connection::open(&t1_analytics_path).unwrap();
        let _ = conn1.execute(
            "INSERT INTO visits (id, target_type, target_id, timestamp, ip_address, country, referer, user_agent, accept_language, status_code) 
             VALUES ('v1', 'url', 'url1', '2026-08-20T10:00:00Z', '1.1.1.1', 'US', 'direct', 'ua', 'en', 301);",
            [],
        ).unwrap();
        let _ = conn1.execute(
            "INSERT INTO visits (id, target_type, target_id, timestamp, ip_address, country, referer, user_agent, accept_language, status_code) 
             VALUES ('v2', 'url', 'url1', '2026-08-20T10:01:00Z', '1.1.1.2', 'US', 'direct', 'ua', 'en', 301);",
            [],
        ).unwrap();
    }

    {
        let conn2 = rusqlite::Connection::open(&t2_analytics_path).unwrap();
        let _ = conn2.execute(
            "INSERT INTO visits (id, target_type, target_id, timestamp, ip_address, country, referer, user_agent, accept_language, status_code) 
             VALUES ('v3', 'url', 'url2', '2026-08-20T10:02:00Z', '2.2.2.2', 'CA', 'direct', 'ua', 'en', 301);",
            [],
        ).unwrap();
    }

    // 3. Aggregate platform total clicks
    let total_clicks = {
        let users_conn = db.users.lock().unwrap();
        bzod::db::users::get_platform_total_clicks(&db.topology, &users_conn).unwrap()
    };

    assert_eq!(
        total_clicks, 3,
        "Platform clicks must aggregate across all active tenant databases"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_admin_resource_inspection_and_moderation() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_p5_mod_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());

    let db = Db::init(&config).expect("Db::init failed");
    let state = build_test_state(&db, &config);

    // 1. Create standard tenant user
    let _ = bzod::cli::create_user::run(
        Some("content_author".to_string()),
        Some("pass123".to_string()),
        None,
        config.clone(),
    )
    .await
    .unwrap();

    let (author_id, author_tid) = {
        let conn = db.users.lock().unwrap();
        let u = bzod::db::users::get_user_by_username(&conn, "content_author")
            .unwrap()
            .unwrap();
        (u.id, u.tenant_id.unwrap())
    };

    // 2. Author creates URL
    let url_id = {
        let conn = bzod::jobs::open_user_content_conn(&db, author_id).unwrap();
        let u = bzod::db::content::create_url_extended(
            &conn,
            "!moderated-link",
            "https://bad-site.com",
            Some("Spam Link"),
            None,
            &[],
            None,
            None,
            None,
        )
        .unwrap();

        let urls_conn = db.global_urls.lock().unwrap();
        bzod::db::slugs::register_url_slug(
            &urls_conn,
            "!moderated-link",
            &author_tid,
            &u.id,
            "active",
        )
        .unwrap();
        u.id
    };

    // 3. Verify Admin can look up slug and inspect owner tenant
    let slug_info = state
        .lookup_slug("!moderated-link")
        .unwrap()
        .expect("Slug must exist in global registry");
    assert_eq!(slug_info.owner_tenant_id, author_tid.as_str());

    // 4. Admin opens tenant content DB in CoreJob mode to inspect
    let tenant_dbs = state
        .open_tenant(author_tid, bzod::state::TenantOpenMode::CoreJob)
        .unwrap();
    {
        let conn = tenant_dbs.content.lock().unwrap();
        let fetched_url = bzod::db::content::get_url_by_id(&conn, &url_id)
            .unwrap()
            .expect("URL must exist in tenant content.db");
        assert_eq!(fetched_url.destination, "https://bad-site.com");
    }

    // 5. Admin retires/moderates the slug
    {
        let urls_conn = db.global_urls.lock().unwrap();
        let pages_conn = db.global_landing_pages.lock().unwrap();
        let retired =
            bzod::db::slugs::retire_slug(&urls_conn, &pages_conn, "!moderated-link").unwrap();
        assert!(retired);
    }

    // 6. Verify slug cannot be re-registered by another tenant
    let _other_tid = TenantId::generate();
    let r_conn = db.reserved.lock().unwrap();
    let u_conn = db.global_urls.lock().unwrap();
    let p_conn = db.global_landing_pages.lock().unwrap();
    let available =
        bzod::db::slugs::is_slug_available(&r_conn, &u_conn, &p_conn, "!moderated-link").unwrap();
    assert!(
        !available,
        "Retired slug must remain unavailable across the platform"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}
