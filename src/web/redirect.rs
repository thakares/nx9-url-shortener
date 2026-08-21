//! Public short-code redirect hot path.
//!
//! Design notes:
//! - Destination `Location` headers never panic on malformed values.
//! - Content DB work uses a single mutex acquisition where safe.
//! - Expiration is enforced on the read path; persistent `expired=1` is left to
//!   the background expiry job (`jobs::expiry`), not written here.
//! - Blocking rusqlite work runs in `spawn_blocking` so Tokio workers are not starved.
//! - Analytics enqueue remains non-blocking (`try_send` via the queue).

use axum::{
    extract::{ConnectInfo, Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;
use chrono::Utc;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tracing::{error, warn};
use uuid::Uuid;

use crate::analytics::get_client_country;
use crate::db::tenant::TenantOpenMode;
use crate::identity::TenantId;
use crate::models::{LinkPreview, Url, VisitRecord};
use crate::state::AppState;
use crate::templates::PreviewTemplate;
use crate::utils::get_client_ip;

struct ResolvedUrl {
    owner_tenant_id: Option<TenantId>,
    owner_user_id: i64,
    url: Url,
    content: Arc<Mutex<rusqlite::Connection>>,
}

/// Compact outcome from blocking redirect DB work (avoids large enum / Result variants).
enum ResolveOutcome {
    Ready(Box<ResolvedUrl>),
    /// Early HTTP response that does not need further processing.
    Early {
        status: StatusCode,
        body: &'static str,
    },
    /// Permanent redirect to a relative path (e.g. page target → `/p/{code}`).
    PermanentPath(String),
    DbError {
        operation: &'static str,
        message: String,
        owner_user_id: Option<i64>,
        resource_id: Option<String>,
    },
}

/// Safe client-facing DB error after structured server-side logging.
fn db_error_response(
    operation: &str,
    code: &str,
    owner_user_id: Option<i64>,
    resource_id: Option<&str>,
    err: impl std::fmt::Display,
) -> Response {
    error!(
        operation = operation,
        code = code,
        owner_user_id = owner_user_id,
        resource_id = resource_id.unwrap_or(""),
        error = %err,
        "redirect path database error"
    );
    (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response()
}

/// Build a permanent redirect without panicking on invalid destinations.
///
/// Defense in depth:
/// 1. Canonical destination rules (http/https, no control chars) — same as writes.
/// 2. `HeaderValue` construction — rejects remaining illegal header bytes.
///
/// Neither step may panic. Full destination values are not logged.
fn permanent_redirect_to(destination: &str, code: &str) -> Response {
    if !crate::utils::validation::validate_redirect_destination(destination) {
        warn!(
            operation = "validate_redirect_destination",
            code = code,
            destination_len = destination.len(),
            "invalid stored redirect destination rejected"
        );
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Invalid redirect destination",
        )
            .into_response();
    }

    match HeaderValue::from_str(destination) {
        Ok(loc) => {
            let mut resp = (StatusCode::MOVED_PERMANENTLY, "").into_response();
            resp.headers_mut().insert(header::LOCATION, loc);
            resp
        }
        Err(err) => {
            warn!(
                operation = "build_location_header",
                code = code,
                error = %err,
                // Do not log the full destination if it may contain control chars;
                // log length only for forensics.
                destination_len = destination.len(),
                "invalid redirect destination rejected (possible response-splitting attempt)"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Invalid redirect destination",
            )
                .into_response()
        }
    }
}

/// Lookup global slug namespace then load the URL from the tenant content DB.
/// Runs entirely on a blocking thread.
fn resolve_url_blocking(
    _system_db: Arc<Mutex<rusqlite::Connection>>,
    state: AppState,
    code: &str,
) -> ResolveOutcome {
    // 1. Query global slug namespace across v0.8 slug databases (with fallback)
    let slug_info = match state.lookup_slug(code) {
        Ok(info) => info,
        Err(e) => {
            return ResolveOutcome::DbError {
                operation: "lookup_slug",
                message: e.to_string(),
                owner_user_id: None,
                resource_id: None,
            };
        }
    };

    let info = match slug_info {
        Some(info) => info,
        None => {
            return ResolveOutcome::Early {
                status: StatusCode::NOT_FOUND,
                body: "Not Found",
            };
        }
    };

    let owner_user_id = info.owner_tenant_id.parse::<i64>().unwrap_or(0);

    // If slug status is disabled, flagged, or soft_deleted, we return 410 Gone
    if info.status != "active" {
        return ResolveOutcome::Early {
            status: StatusCode::GONE,
            body: "This content has been disabled or moderated",
        };
    }

    // If target type is page, redirect permanently to /p/slug
    if info.target_type == crate::db::slugs::SlugTargetType::LandingPage {
        return ResolveOutcome::PermanentPath(format!("/p/{}", code));
    }

    // 2. Get content database connection via tenant DB resolution
    let content_conn =
        match state.open_slug_owner(&info.owner_tenant_id, TenantOpenMode::PublicContent) {
            Ok(dbs) => dbs,
            Err(e) => {
                return ResolveOutcome::DbError {
                    operation: "open_slug_owner",
                    message: e.to_string(),
                    owner_user_id: Some(owner_user_id),
                    resource_id: None,
                };
            }
        };

    let url = {
        let conn = match content_conn.content.lock() {
            Ok(c) => c,
            Err(e) => {
                return ResolveOutcome::DbError {
                    operation: "lock_content_db",
                    message: e.to_string(),
                    owner_user_id: Some(owner_user_id),
                    resource_id: None,
                };
            }
        };
        match crate::db::content::get_url_by_code(&conn, code) {
            Ok(Some(url)) => url,
            Ok(None) => {
                return ResolveOutcome::Early {
                    status: StatusCode::NOT_FOUND,
                    body: "Short code not found",
                };
            }
            Err(e) => {
                return ResolveOutcome::DbError {
                    operation: "get_url_by_code",
                    message: e.to_string(),
                    owner_user_id: Some(owner_user_id),
                    resource_id: None,
                };
            }
        }
    };

    let owner_tenant_id = TenantId::parse(&info.owner_tenant_id).ok();

    ResolveOutcome::Ready(Box::new(ResolvedUrl {
        owner_tenant_id,
        owner_user_id,
        url,
        content: content_conn.content,
    }))
}

/// Increment access count and load preview under a single content-DB lock.
fn increment_and_preview_blocking(
    content: Arc<Mutex<rusqlite::Connection>>,
    url_id: &str,
    code: &str,
    owner_user_id: i64,
    fallback_access_count: i64,
) -> Result<(i64, Option<LinkPreview>), String> {
    let conn = content
        .lock()
        .map_err(|e| format!("lock_content_db_hot: {}", e))?;

    let new_access_count = match crate::db::content::increment_access_count(&conn, url_id) {
        Ok(n) => n,
        Err(e) => {
            // Preserve prior soft-failure semantics for the counter value used only
            // internally; never silence the underlying error.
            error!(
                operation = "increment_access_count",
                code = code,
                owner_user_id = owner_user_id,
                resource_id = url_id,
                error = %e,
                "failed to increment access count; continuing with estimated value"
            );
            fallback_access_count + 1
        }
    };

    let preview = match crate::db::preview::get_preview(&conn, url_id) {
        Ok(p) => p,
        Err(e) => {
            error!(
                operation = "get_preview",
                code = code,
                owner_user_id = owner_user_id,
                resource_id = url_id,
                error = %e,
                "failed to load link preview; continuing without preview"
            );
            None
        }
    };

    Ok((new_access_count, preview))
}

// GET /:code
// Resolve and redirect
pub async fn resolve_redirect(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(code): Path<String>,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
) -> Response {
    // Basic validation of code (must be 6 hex characters, 4 hex characters, or a valid custom slug)
    if !crate::utils::validation::validate_redirect_code(&code)
        && !crate::utils::validation::validate_page_code(&code)
    {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    }

    let system_db = state.system_db.clone();
    let state_for_lookup = state.clone();
    let code_for_lookup = code.clone();

    let outcome = match tokio::task::spawn_blocking(move || {
        resolve_url_blocking(system_db, state_for_lookup, &code_for_lookup)
    })
    .await
    {
        Ok(outcome) => outcome,
        Err(e) => {
            return db_error_response("spawn_blocking_resolve", &code, None, None, e.to_string());
        }
    };

    let resolved = match outcome {
        ResolveOutcome::Ready(r) => r,
        ResolveOutcome::Early { status, body } => return (status, body).into_response(),
        ResolveOutcome::PermanentPath(path) => {
            return Redirect::permanent(&path).into_response();
        }
        ResolveOutcome::DbError {
            operation,
            message,
            owner_user_id,
            resource_id,
        } => {
            return db_error_response(
                operation,
                &code,
                owner_user_id,
                resource_id.as_deref(),
                message,
            );
        }
    };

    let ResolvedUrl {
        owner_tenant_id,
        owner_user_id,
        url,
        content,
    } = *resolved;

    // 3. Expiration check (read-only on the hot path).
    // Background job `jobs::expiry::run_expiry_checker` persists expired=1.
    if url.expired {
        return (StatusCode::GONE, "This link has expired").into_response();
    }

    if let Some(ref expires_at_str) = url.expires_at {
        if let Ok(expires_at) = chrono::DateTime::parse_from_rfc3339(expires_at_str) {
            if expires_at.with_timezone(&Utc) < Utc::now() {
                return (StatusCode::GONE, "This link has expired").into_response();
            }
        }
    }

    // 4. Access limit check
    if url.is_access_exhausted() {
        return (
            StatusCode::GONE,
            "This link has reached its maximum access limit",
        )
            .into_response();
    }

    // 5. Password protection check
    if url.is_password_protected() {
        let cookie_name = format!("bzod_gate_{}", code);
        let authorized = jar
            .get(&cookie_name)
            .map(|c| c.value() == "authorized")
            .unwrap_or(false);

        if !authorized {
            return Redirect::temporary(&format!("/gate/{}", code)).into_response();
        }
    }

    // 6. Increment access count & retrieve preview config (single content lock, off executor)
    let url_id = url.id.clone();
    let code_for_hot = code.clone();
    let fallback_access_count = url.access_count;
    let preview_opt = match tokio::task::spawn_blocking(move || {
        increment_and_preview_blocking(
            content,
            &url_id,
            &code_for_hot,
            owner_user_id,
            fallback_access_count,
        )
    })
    .await
    {
        Ok(Ok((_new_access_count, preview))) => preview,
        Ok(Err(msg)) => {
            return db_error_response(
                "increment_and_preview",
                &code,
                Some(owner_user_id),
                Some(&url.id),
                msg,
            );
        }
        Err(e) => {
            return db_error_response(
                "spawn_blocking_hot",
                &code,
                Some(owner_user_id),
                Some(&url.id),
                e.to_string(),
            );
        }
    };

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

    let record = VisitRecord {
        id: Uuid::new_v4().to_string(),
        target_type: "url".to_string(),
        target_id: url.id.clone(),
        timestamp: Utc::now().to_rfc3339(),
        ip_address: ip,
        user_agent,
        referer,
        accept_language,
        country,
        status_code: if preview_opt.is_some() { 200 } else { 302 },
        owner_tenant_id,
        owner_user_id: Some(owner_user_id),
    };

    // Push to memory queue (non-blocking)
    state.analytics_queue.push(record);

    // 7. Render Preview or Redirect
    if let Some(preview) = preview_opt {
        PreviewTemplate {
            code,
            title: preview.title,
            description: preview.description,
            logo_url: preview.logo_url,
            button_text: preview.button_text,
            destination: url.destination,
        }
        .into_response()
    } else {
        permanent_redirect_to(&url.destination, &code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permanent_redirect_rejects_crlf() {
        let resp = permanent_redirect_to("https://evil.example/\r\nX-Injected: yes", "abc123");
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(resp.headers().get(header::LOCATION).is_none());
    }

    #[test]
    fn permanent_redirect_rejects_control_chars() {
        let resp = permanent_redirect_to("https://evil.example/\x00payload", "abc123");
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn permanent_redirect_accepts_valid_url() {
        let resp = permanent_redirect_to("https://example.com/path?q=1", "abc123");
        assert_eq!(resp.status(), StatusCode::MOVED_PERMANENTLY);
        let loc = resp.headers().get(header::LOCATION).unwrap();
        assert_eq!(loc, "https://example.com/path?q=1");
    }
}
