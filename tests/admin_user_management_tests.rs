use axum::extract::{Form, Query, State};
use axum::http::HeaderMap;
use axum_extra::extract::cookie::{Cookie, CookieJar};
use chrono::Utc;
use rusqlite::Connection;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use bzod::analytics::AnalyticsQueue;
use bzod::auth::hash_password;
use bzod::config::Config;
use bzod::db::users::{create_admin_user, get_user_by_username};
use bzod::db::Db;
use bzod::state::AppState;
use bzod::web::admin::{
    users_create_post, users_delete_post, users_get, CreateUserForm, DeleteUserForm, UsersQuery,
};

fn create_temp_config(temp_dir: PathBuf) -> Config {
    let mut config = Config::load();
    config.data_dir = temp_dir.clone();
    config.backup_dir = temp_dir.clone();
    config.base_url = Some("http://localhost:8080".to_string());
    config
}

fn build_state(config: Config) -> (Db, AppState) {
    let db = Db::init(&config).expect("Failed to init Db");
    let (tx, rx) = tokio::sync::watch::channel(false);
    Box::leak(Box::new(tx));
    let (queue, _) = AnalyticsQueue::new(db.clone(), 1000, rx);
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
    (db, state)
}

fn create_admin_session(conn: &Connection, admin_id: i64, session_id: &str) {
    let now = Utc::now();
    let expires_at = (now + chrono::Duration::hours(1)).to_rfc3339();
    let created_at = now.to_rfc3339();
    conn.execute(
        "INSERT INTO sessions (id, user_id, expires_at, created_at) VALUES (?1, ?2, ?3, ?4);",
        rusqlite::params![session_id, admin_id, expires_at, created_at],
    )
    .expect("Failed to insert admin session");
}

#[tokio::test]
async fn test_admin_users_page_and_user_creation() {
    let temp_dir = std::env::temp_dir().join(format!(
        "bzod_test_admin_user_management_{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let (_db, state) = build_state(config.clone());

    let password = "adminpassword";
    let hash = hash_password(password).unwrap();
    let admin_user = {
        let conn = state.users_db.lock().unwrap();
        create_admin_user(&conn, "admin", &hash).unwrap()
    };

    let session_token = "session-token-123";
    {
        let conn = state.users_db.lock().unwrap();
        create_admin_session(&conn, admin_user.id, session_token);
    }

    let jar = CookieJar::new().add(Cookie::new("bzod_session", session_token));
    let response = users_get(
        State(state.clone()),
        jar,
        Query(UsersQuery {
            success: None,
            error: None,
        }),
    )
    .await;

    if response.status().is_redirection() {
        let location = response
            .headers()
            .get("location")
            .map(|v| v.to_str().unwrap_or(""));
        println!("users_get redirect to: {:?}", location);
    }

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("Users Management"));
    assert!(body_str.contains("Create New User"));
    assert!(body_str.contains("admin"));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_admin_create_and_delete_user() {
    let temp_dir = std::env::temp_dir().join(format!(
        "bzod_test_admin_user_management_delete_{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&temp_dir).unwrap();
    let config = create_temp_config(temp_dir.clone());
    let (_db, state) = build_state(config.clone());

    let password = "adminpassword";
    let hash = hash_password(password).unwrap();
    let admin_user = {
        let conn = state.users_db.lock().unwrap();
        create_admin_user(&conn, "admin", &hash).unwrap()
    };

    let session_token = "session-token-456";
    {
        let conn = state.users_db.lock().unwrap();
        create_admin_session(&conn, admin_user.id, session_token);
    }

    let jar = CookieJar::new().add(Cookie::new("bzod_session", session_token));

    let form = CreateUserForm {
        username: "testuser".to_string(),
        password: "password123".to_string(),
        account_type: "standard".to_string(),
        metadata: "test metadata".to_string(),
        csrf_token: "invalid".to_string(),
    };

    // invalid CSRF should redirect with error
    let response = users_create_post(State(state.clone()), jar.clone(), Form(form)).await;
    assert!(response.status().is_redirection());
    assert!(response
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("Invalid CSRF token"));

    // build a valid CSRF token from admin session
    let csrf_token = bzod::auth::generate_csrf_token(session_token);
    let form = CreateUserForm {
        username: "testuser".to_string(),
        password: "password123".to_string(),
        account_type: "standard".to_string(),
        metadata: "test metadata".to_string(),
        csrf_token: csrf_token.clone(),
    };

    let response = users_create_post(State(state.clone()), jar.clone(), Form(form)).await;
    assert!(response.status().is_redirection());
    assert_eq!(
        response.headers().get("location").unwrap(),
        "/admin/users?success=User created successfully"
    );

    let created_user_id = {
        let conn = state.users_db.lock().unwrap();
        let user = get_user_by_username(&conn, "testuser")
            .unwrap()
            .expect("created user should exist");
        user.id
    };

    let delete_form = DeleteUserForm {
        csrf_token: csrf_token.clone(),
    };
    let response = users_delete_post(
        State(state.clone()),
        jar.clone(),
        HeaderMap::new(),
        None,
        axum::extract::Path(created_user_id),
        Form(delete_form),
    )
    .await;

    assert!(response.status().is_redirection());
    assert_eq!(
        response.headers().get("location").unwrap(),
        "/admin/users?success=User deleted successfully"
    );

    {
        let conn = state.users_db.lock().unwrap();
        let user = get_user_by_username(&conn, "testuser").unwrap();
        assert!(user.is_none());
    }

    let _ = fs::remove_dir_all(&temp_dir);
}
