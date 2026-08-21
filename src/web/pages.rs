use axum::{
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
};
use chrono::Utc;
use std::net::SocketAddr;
use uuid::Uuid;

use crate::analytics::get_client_country;
use crate::db::tenant::TenantOpenMode;
use crate::models::VisitRecord;
use crate::state::AppState;
use crate::utils::get_client_ip;

// GET /p/:code and GET /p/:code/*slug
// Resolve and render landing page
pub async fn resolve_page(
    State(state): State<AppState>,
    Path(code): Path<String>,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
) -> Response {
    if !crate::utils::validation::validate_page_code(&code) {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    }

    // 1. Query global slug namespace across v0.8 slug databases (with fallback)
    let info = match state.lookup_slug(&code) {
        Ok(Some(info)) => info,
        Ok(None) => return (StatusCode::NOT_FOUND, "Not Found").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    };

    // If slug status is disabled, flagged, or soft_deleted, we return 410 Gone
    if info.status != "active" {
        return (
            StatusCode::GONE,
            "This content has been disabled or moderated",
        )
            .into_response();
    }

    // 2. Get content database connection via tenant DB resolution
    let content_conn =
        match state.open_slug_owner(&info.owner_tenant_id, TenantOpenMode::PublicContent) {
            Ok(dbs) => dbs.content,
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
        };

    let page_opt = {
        let conn = content_conn.lock().unwrap();
        match crate::db::content::get_landing_page_by_code(&conn, &code) {
            Ok(page) => page,
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
        }
    };

    match page_opt {
        Some(page) => {
            // Check state
            if page.state == "archived" {
                return (StatusCode::GONE, "This landing page has been archived").into_response();
            }

            // Record view analytics
            let ip = get_client_ip(&headers, connect_info);
            let country = get_client_country(&headers);
            let user_agent = headers
                .get("user-agent")
                .and_then(|h| h.to_str().ok())
                .unwrap_or("Unknown")
                .to_string();
            let referer = headers
                .get("referer")
                .and_then(|h| h.to_str().ok())
                .unwrap_or("Direct")
                .to_string();
            let accept_language = headers
                .get("accept-language")
                .and_then(|h| h.to_str().ok())
                .unwrap_or("Unknown")
                .to_string();

            let owner_tenant_id = crate::identity::TenantId::parse(&info.owner_tenant_id).ok();
            let owner_user_id = info.owner_tenant_id.parse::<i64>().ok();

            let record = VisitRecord {
                id: Uuid::new_v4().to_string(),
                target_type: "page".to_string(),
                target_id: page.id.clone(),
                timestamp: Utc::now().to_rfc3339(),
                ip_address: ip,
                user_agent,
                referer,
                accept_language,
                country,
                status_code: 200,
                owner_tenant_id,
                owner_user_id,
            };

            state.analytics_queue.push(record);

            // Render raw HTML
            Html(page.html_content).into_response()
        }
        None => (StatusCode::NOT_FOUND, "Landing page not found").into_response(),
    }
}

// GET /
// Serve static root landing page from www/index.html
pub async fn root_landing() -> Response {
    let mut target_path = std::path::PathBuf::from("www/index.html");

    if !target_path.exists() {
        // Search relative to executable
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let path1 = exe_dir.join("www/index.html");
                if path1.exists() {
                    target_path = path1;
                } else if let Some(parent1) = exe_dir.parent() {
                    let path2 = parent1.join("www/index.html");
                    if path2.exists() {
                        target_path = path2;
                    } else if let Some(parent2) = parent1.parent() {
                        let path3 = parent2.join("www/index.html");
                        if path3.exists() {
                            target_path = path3;
                        }
                    }
                }
            }
        }
    }

    if !target_path.exists() {
        // Search in CARGO_MANIFEST_DIR
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let path = std::path::PathBuf::from(manifest_dir).join("www/index.html");
            if path.exists() {
                target_path = path;
            }
        }
    }

    match std::fs::read_to_string(&target_path) {
        Ok(content) => Html(content).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "Not Found").into_response(),
    }
}

pub async fn deploy_script() -> Response {
    match tokio::fs::read("www/deploy.sh").await {
        Ok(content) => (
            StatusCode::OK,
            [("content-type", "text/plain; charset=utf-8")],
            content,
        )
            .into_response(),

        Err(_) => (StatusCode::NOT_FOUND, "deploy.sh not found").into_response(),
    }
}

pub async fn social_preview() -> Response {
    let mut target_path = std::path::PathBuf::from("www/images/preview.png");

    if !target_path.exists() {
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let path1 = exe_dir.join("www/images/preview.png");
                if path1.exists() {
                    target_path = path1;
                } else if let Some(parent1) = exe_dir.parent() {
                    let path2 = parent1.join("www/images/preview.png");
                    if path2.exists() {
                        target_path = path2;
                    } else if let Some(parent2) = parent1.parent() {
                        let path3 = parent2.join("www/images/preview.png");
                        if path3.exists() {
                            target_path = path3;
                        }
                    }
                }
            }
        }
    }

    if !target_path.exists() {
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let path = std::path::PathBuf::from(manifest_dir).join("www/images/preview.png");
            if path.exists() {
                target_path = path;
            }
        }
    }

    match tokio::fs::read(target_path).await {
        Ok(content) => (
            StatusCode::OK,
            [
                ("content-type", "image/png"),
                ("cache-control", "public, max-age=86400"),
            ],
            content,
        )
            .into_response(),

        Err(_) => (StatusCode::NOT_FOUND, "preview.png not found").into_response(),
    }
}
