//! Redirect security & behavioral regression tests (Phase 2).
//!
//! Covers: 301, legacy malicious destinations, expiration, access limits,
//! password gate ordering, previews, tenant slug isolation, concurrent access.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;
use tokio::sync::Barrier;

use bzod::analytics::AnalyticsQueue;
use bzod::auth::password::hash_password;
use bzod::config::Config;
use bzod::db::Db;
use bzod::services::destination_audit::{
    audit_all_destinations, audit_content_conn, DestinationAuditReport,
};
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
) -> (reqwest::Client, String, Db, tokio::task::JoinHandle<()>) {
    let config = create_temp_config(temp_dir);
    let db = Db::init(&config).expect("Failed to init Db");
    let (tx, rx) = tokio::sync::watch::channel(false);
    Box::leak(Box::new(tx));
    let (queue, _) = AnalyticsQueue::new(db.clone(), 1000, rx);

    // Create a standard tenant user
    {
        let users = db.users.lock().unwrap();
        let _ = bzod::db::users::create_user(&users, "testuser", "dummy_hash", "standard", None);
    }
    let _ = db.init_user_databases(1);

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
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    (client, url, db, handle)
}

struct SeedUrl<'a> {
    owner_user_id: i64,
    code: &'a str,
    destination: &'a str,
    expired: bool,
    expires_at: Option<&'a str>,
    password_hash: Option<&'a str>,
    max_access_count: Option<i64>,
    access_count: i64,
}

fn seed_url(db: &Db, seed: SeedUrl<'_>) {
    // Ensure user content DB exists
    db.init_user_databases(seed.owner_user_id)
        .expect("init user dbs");

    let (content_path, tid) = {
        let users = db.users.lock().unwrap();
        let user = bzod::db::users::get_user_by_id(&users, seed.owner_user_id)
            .unwrap()
            .expect("seed owner must be a registered user");
        let path = bzod::db::tenant::location_for_user(&user)
            .unwrap()
            .dir(&db.topology)
            .unwrap()
            .join("content.db");
        (path, user.tenant_id.unwrap())
    };

    let id = uuid::Uuid::new_v4().to_string();

    {
        let urls_conn = db.global_urls.lock().unwrap();
        let _ = bzod::db::slugs::register_url_slug(&urls_conn, seed.code, &tid, &id, "active");
    }

    let conn = rusqlite::Connection::open(content_path).unwrap();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO urls (id, code, destination, title, description, status, created_at, updated_at, expires_at, expired, password_hash, max_access_count, access_count)
         VALUES (?1, ?2, ?3, NULL, NULL, 'healthy', ?4, ?4, ?5, ?6, ?7, ?8, ?9);",
        rusqlite::params![
            id,
            seed.code,
            seed.destination,
            now,
            seed.expires_at,
            if seed.expired { 1 } else { 0 },
            seed.password_hash,
            seed.max_access_count,
            seed.access_count,
        ],
    )
    .unwrap();
}

#[tokio::test]
async fn valid_destination_returns_301() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_redir_301_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let (client, base, db, _h) = start_test_server(temp_dir.clone()).await;

    seed_url(
        &db,
        SeedUrl {
            owner_user_id: 1,
            code: "a1b2c3",
            destination: "https://example.com/target",
            expired: false,
            expires_at: None,
            password_hash: None,
            max_access_count: None,
            access_count: 0,
        },
    );

    let res = client.get(format!("{}/a1b2c3", base)).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::MOVED_PERMANENTLY);
    assert_eq!(
        res.headers().get("location").unwrap().to_str().unwrap(),
        "https://example.com/target"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn legacy_crlf_destination_fails_closed_no_location() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_redir_crlf_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let (client, base, db, _h) = start_test_server(temp_dir.clone()).await;

    // Simulate legacy DB row that bypassed modern write validation.
    seed_url(
        &db,
        SeedUrl {
            owner_user_id: 1,
            code: "dead01",
            destination: "https://evil.example/\r\nX-Injected: yes",
            expired: false,
            expires_at: None,
            password_hash: None,
            max_access_count: None,
            access_count: 0,
        },
    );

    let res = client.get(format!("{}/dead01", base)).send().await.unwrap();
    // Must not panic; fail closed without Location header.
    assert_eq!(res.status(), reqwest::StatusCode::INTERNAL_SERVER_ERROR);
    assert!(res.headers().get("location").is_none());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn javascript_scheme_legacy_fails_closed() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_redir_js_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let (client, base, db, _h) = start_test_server(temp_dir.clone()).await;

    seed_url(
        &db,
        SeedUrl {
            owner_user_id: 1,
            code: "ab1111",
            destination: "javascript:alert(1)",
            expired: false,
            expires_at: None,
            password_hash: None,
            max_access_count: None,
            access_count: 0,
        },
    );

    let res = client.get(format!("{}/ab1111", base)).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::INTERNAL_SERVER_ERROR);
    assert!(res.headers().get("location").is_none());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn expired_flag_returns_410_without_redirect() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_redir_exp_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let (client, base, db, _h) = start_test_server(temp_dir.clone()).await;

    seed_url(
        &db,
        SeedUrl {
            owner_user_id: 1,
            code: "ab2222",
            destination: "https://example.com/gone",
            expired: true,
            expires_at: None,
            password_hash: None,
            max_access_count: None,
            access_count: 0,
        },
    );

    let res = client.get(format!("{}/ab2222", base)).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::GONE);
    assert!(res.headers().get("location").is_none());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn wall_clock_expiry_returns_410_without_hot_path_write_dependency() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_redir_exp2_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let (client, base, db, _h) = start_test_server(temp_dir.clone()).await;

    // expired=0 but expires_at in the past → still 410 (read-path authoritative).
    seed_url(
        &db,
        SeedUrl {
            owner_user_id: 1,
            code: "ab3333",
            destination: "https://example.com/gone2",
            expired: false,
            expires_at: Some("2000-01-01T00:00:00Z"),
            password_hash: None,
            max_access_count: None,
            access_count: 0,
        },
    );

    let res = client.get(format!("{}/ab3333", base)).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::GONE);

    // Column may still be 0 until sweeper runs — correctness does not require write.
    let content_path = {
        let users = db.users.lock().unwrap();
        let user = bzod::db::users::get_user_by_id(&users, 1).unwrap().unwrap();
        bzod::db::tenant::location_for_user(&user)
            .unwrap()
            .dir(&db.topology)
            .unwrap()
            .join("content.db")
    };
    let conn = rusqlite::Connection::open(content_path).unwrap();
    let expired_flag: i64 = conn
        .query_row("SELECT expired FROM urls WHERE code = 'ab3333';", [], |r| {
            r.get(0)
        })
        .unwrap();
    // Either 0 (hot path no write) or 1 is acceptable if something else flipped it;
    // the important assertion is 410 above. Prefer documenting no write:
    assert!(
        expired_flag == 0 || expired_flag == 1,
        "unexpected expired flag {}",
        expired_flag
    );
    // Phase 2 guarantee: no hot-path write required — if still 0, sweeper is maintenance only.
    assert_eq!(
        expired_flag, 0,
        "redirect hot path must not persist expired=1"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn access_limit_exhausted_returns_410() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_redir_lim_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let (client, base, db, _h) = start_test_server(temp_dir.clone()).await;

    seed_url(
        &db,
        SeedUrl {
            owner_user_id: 1,
            code: "ab4444",
            destination: "https://example.com/limited",
            expired: false,
            expires_at: None,
            password_hash: None,
            max_access_count: Some(2),
            access_count: 2, // already exhausted
        },
    );

    let res = client.get(format!("{}/ab4444", base)).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::GONE);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn password_gate_before_access_increment() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_redir_pw_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let (client, base, db, _h) = start_test_server(temp_dir.clone()).await;

    let hash = hash_password("secret-pass").unwrap();
    seed_url(
        &db,
        SeedUrl {
            owner_user_id: 1,
            code: "ab5555",
            destination: "https://example.com/secret",
            expired: false,
            expires_at: None,
            password_hash: Some(&hash),
            max_access_count: None,
            access_count: 0,
        },
    );

    let res = client.get(format!("{}/ab5555", base)).send().await.unwrap();
    assert!(res.status().is_redirection());
    let loc = res.headers().get("location").unwrap().to_str().unwrap();
    assert!(loc.contains("/gate/ab5555"));

    // Access count must not have incremented
    let content_path = {
        let users = db.users.lock().unwrap();
        let user = bzod::db::users::get_user_by_id(&users, 1).unwrap().unwrap();
        bzod::db::tenant::location_for_user(&user)
            .unwrap()
            .dir(&db.topology)
            .unwrap()
            .join("content.db")
    };
    let conn = rusqlite::Connection::open(content_path).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT access_count FROM urls WHERE code = 'ab5555';",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn concurrent_redirects_increment_access_count() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_redir_conc_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let (client, base, db, _h) = start_test_server(temp_dir.clone()).await;

    seed_url(
        &db,
        SeedUrl {
            owner_user_id: 1,
            code: "ab6666",
            destination: "https://example.com/concurrent",
            expired: false,
            expires_at: None,
            password_hash: None,
            max_access_count: None,
            access_count: 0,
        },
    );

    const N: usize = 20;
    let barrier = Arc::new(Barrier::new(N));
    let mut handles = Vec::new();
    for _ in 0..N {
        let client = client.clone();
        let url = format!("{}/ab6666", base);
        let b = barrier.clone();
        handles.push(tokio::spawn(async move {
            b.wait().await;
            client.get(&url).send().await.unwrap()
        }));
    }

    let mut ok_301 = 0;
    for h in handles {
        let res = h.await.unwrap();
        if res.status() == reqwest::StatusCode::MOVED_PERMANENTLY {
            ok_301 += 1;
        }
    }
    assert_eq!(ok_301, N);

    let content_path = {
        let users = db.users.lock().unwrap();
        let user = bzod::db::users::get_user_by_id(&users, 1).unwrap().unwrap();
        bzod::db::tenant::location_for_user(&user)
            .unwrap()
            .dir(&db.topology)
            .unwrap()
            .join("content.db")
    };
    let conn = rusqlite::Connection::open(content_path).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT access_count FROM urls WHERE code = 'ab6666';",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        count, N as i64,
        "each successful redirect should increment access_count once"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn tenant_slug_resolves_owner_not_cross_tenant_content() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_redir_ten_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let (client, base, db, _h) = start_test_server(temp_dir.clone()).await;

    // Create two users
    let config = create_temp_config(temp_dir.clone());
    let _ = bzod::cli::create_user::run(
        Some("alice".into()),
        Some("password123".into()),
        None,
        config.clone(),
    )
    .await;
    let _ = bzod::cli::create_user::run(
        Some("bob".into()),
        Some("password123".into()),
        None,
        config.clone(),
    )
    .await;

    let (alice_id, bob_id) = {
        let conn = db.users.lock().unwrap();
        let a = bzod::db::users::get_user_by_username(&conn, "alice")
            .unwrap()
            .unwrap()
            .id;
        let b = bzod::db::users::get_user_by_username(&conn, "bob")
            .unwrap()
            .unwrap()
            .id;
        (a, b)
    };

    seed_url(
        &db,
        SeedUrl {
            owner_user_id: alice_id,
            code: "!alice1",
            destination: "https://alice.example/ok",
            expired: false,
            expires_at: None,
            password_hash: None,
            max_access_count: None,
            access_count: 0,
        },
    );
    seed_url(
        &db,
        SeedUrl {
            owner_user_id: bob_id,
            code: "!bob001",
            destination: "https://bob.example/ok",
            expired: false,
            expires_at: None,
            password_hash: None,
            max_access_count: None,
            access_count: 0,
        },
    );

    let res_a = client
        .get(format!("{}/!alice1", base))
        .send()
        .await
        .unwrap();
    assert_eq!(res_a.status(), reqwest::StatusCode::MOVED_PERMANENTLY);
    assert_eq!(
        res_a.headers().get("location").unwrap().to_str().unwrap(),
        "https://alice.example/ok"
    );

    let res_b = client
        .get(format!("{}/!bob001", base))
        .send()
        .await
        .unwrap();
    assert_eq!(res_b.status(), reqwest::StatusCode::MOVED_PERMANENTLY);
    assert_eq!(
        res_b.headers().get("location").unwrap().to_str().unwrap(),
        "https://bob.example/ok"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn destination_audit_finds_legacy_invalid_without_rewriting() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_audit_dest_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let db = Db::init(&config).unwrap();
    {
        let users = db.users.lock().unwrap();
        let _ = bzod::db::users::create_user(&users, "testuser", "dummy_hash", "standard", None);
    }

    seed_url(
        &db,
        SeedUrl {
            owner_user_id: 1,
            code: "ab9999",
            destination: "https://example.com/good",
            expired: false,
            expires_at: None,
            password_hash: None,
            max_access_count: None,
            access_count: 0,
        },
    );
    seed_url(
        &db,
        SeedUrl {
            owner_user_id: 1,
            code: "bad001",
            destination: "https://evil/\r\nX:1",
            expired: false,
            expires_at: None,
            password_hash: None,
            max_access_count: None,
            access_count: 0,
        },
    );
    seed_url(
        &db,
        SeedUrl {
            owner_user_id: 1,
            code: "bad002",
            destination: "javascript:alert(1)",
            expired: false,
            expires_at: None,
            password_hash: None,
            max_access_count: None,
            access_count: 0,
        },
    );

    let report = audit_all_destinations(&db).unwrap();
    assert_eq!(report.total_urls, 3);
    assert_eq!(report.valid_https, 1);
    assert_eq!(report.invalid, 2);
    assert_eq!(report.control_characters, 1);
    assert_eq!(report.unsupported_scheme, 1);

    // Data not rewritten
    let content_path = {
        let users = db.users.lock().unwrap();
        let user = bzod::db::users::get_user_by_id(&users, 1).unwrap().unwrap();
        bzod::db::tenant::location_for_user(&user)
            .unwrap()
            .dir(&db.topology)
            .unwrap()
            .join("content.db")
    };
    let conn = rusqlite::Connection::open(content_path).unwrap();
    let dest: String = conn
        .query_row(
            "SELECT destination FROM urls WHERE code = 'bad001';",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(dest.contains('\r'));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn disabled_slug_returns_410() {
    let temp_dir = std::env::temp_dir().join(format!("bzod_redir_dis_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let (client, base, db, _h) = start_test_server(temp_dir.clone()).await;

    seed_url(
        &db,
        SeedUrl {
            owner_user_id: 1,
            code: "ab7777",
            destination: "https://example.com/x",
            expired: false,
            expires_at: None,
            password_hash: None,
            max_access_count: None,
            access_count: 0,
        },
    );
    {
        let urls_conn = db.global_urls.lock().unwrap();
        urls_conn
            .execute(
                "UPDATE global_urls SET status = 'disabled' WHERE slug = 'ab7777';",
                [],
            )
            .unwrap();
    }

    let res = client.get(format!("{}/ab7777", base)).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::GONE);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn redirect_performance_smoke() {
    // Not a CI gate for absolute latency — documents methodology and asserts
    // correctness under concurrent load (error rate = 0, access counts match).
    let temp_dir = std::env::temp_dir().join(format!("bzod_redir_perf_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let (client, base, db, _h) = start_test_server(temp_dir.clone()).await;

    seed_url(
        &db,
        SeedUrl {
            owner_user_id: 1,
            code: "ab8888",
            destination: "https://example.com/perf",
            expired: false,
            expires_at: None,
            password_hash: None,
            max_access_count: None,
            access_count: 0,
        },
    );

    // Warm-up
    for _ in 0..10 {
        let _ = client.get(format!("{}/ab8888", base)).send().await.unwrap();
    }
    // Reset access count after warm-up for clean correctness check
    {
        let content_path = {
            let users = db.users.lock().unwrap();
            let user = bzod::db::users::get_user_by_id(&users, 1).unwrap().unwrap();
            bzod::db::tenant::location_for_user(&user)
                .unwrap()
                .dir(&db.topology)
                .unwrap()
                .join("content.db")
        };
        let conn = rusqlite::Connection::open(content_path).unwrap();
        conn.execute(
            "UPDATE urls SET access_count = 0 WHERE code = 'ab8888';",
            [],
        )
        .unwrap();
    }

    const REQUESTS: usize = 200;
    const CONCURRENCY: usize = 20;
    let mut latencies_ms: Vec<f64> = Vec::with_capacity(REQUESTS);
    let mut errors = 0usize;

    let mut remaining = REQUESTS;
    while remaining > 0 {
        let batch = remaining.min(CONCURRENCY);
        let mut handles = Vec::with_capacity(batch);
        for _ in 0..batch {
            let client = client.clone();
            let url = format!("{}/ab8888", base);
            handles.push(tokio::spawn(async move {
                let start = Instant::now();
                let res = client.get(&url).send().await;
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                (res, elapsed)
            }));
        }
        for h in handles {
            match h.await.unwrap() {
                (Ok(res), ms) if res.status() == reqwest::StatusCode::MOVED_PERMANENTLY => {
                    latencies_ms.push(ms);
                }
                _ => errors += 1,
            }
        }
        remaining -= batch;
    }

    latencies_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p = |q: f64| {
        let idx = ((latencies_ms.len() as f64 - 1.0) * q).round() as usize;
        latencies_ms[idx]
    };
    let p50 = p(0.50);
    let p95 = p(0.95);
    let p99 = p(0.99);
    let throughput = REQUESTS as f64
        / (latencies_ms.iter().sum::<f64>() / CONCURRENCY as f64 / 1000.0).max(0.001);

    eprintln!("=== redirect_performance_smoke ===");
    eprintln!("env: local loopback, SQLite WAL, warm connections");
    eprintln!("requests={}, concurrency={}", REQUESTS, CONCURRENCY);
    eprintln!(
        "p50={:.3}ms p95={:.3}ms p99={:.3}ms errors={} approx_rps={:.1}",
        p50, p95, p99, errors, throughput
    );
    eprintln!("baseline comparison: UNAVAILABLE (no git history in workspace)");

    assert_eq!(errors, 0, "error rate must be zero");
    assert_eq!(latencies_ms.len(), REQUESTS);

    let content_path = {
        let users = db.users.lock().unwrap();
        let user = bzod::db::users::get_user_by_id(&users, 1)
            .unwrap()
            .expect("user 1 must exist");
        bzod::db::tenant::location_for_user(&user)
            .unwrap()
            .dir(&db.topology)
            .unwrap()
            .join("content.db")
    };
    let conn = rusqlite::Connection::open(content_path).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT access_count FROM urls WHERE code = 'ab8888';",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, REQUESTS as i64);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn audit_content_conn_unit_path() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE urls (
            id TEXT PRIMARY KEY,
            code TEXT NOT NULL,
            destination TEXT NOT NULL
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO urls VALUES ('1','a','https://ok.example/');",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO urls VALUES ('2','b','javascript:x');", [])
        .unwrap();

    let mut report = DestinationAuditReport::default();
    audit_content_conn(&conn, 9, &mut report).unwrap();
    assert_eq!(report.total_urls, 2);
    assert_eq!(report.valid_https, 1);
    assert_eq!(report.unsupported_scheme, 1);
}
