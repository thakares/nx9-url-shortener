use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::ApiUser;
use crate::identity::TenantId;
use crate::models::ApiActor;
use crate::state::AppState;

#[derive(Serialize, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub account_type: Option<String>,
    pub metadata: Option<String>,
}

#[derive(Serialize)]
pub struct UserResponse {
    pub id: i64,
    pub username: String,
    pub status: String,
    pub account_type: String,
    pub created_at: String,
    pub metadata: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct UpdateUserStatusRequest {
    pub status: String,
}

#[derive(Serialize, Deserialize)]
pub struct UpdateUserQuotasRequest {
    pub max_urls: i64,
    pub max_landings: i64,
    pub max_api_tokens: i64,
    pub max_storage_mb: i64,
}

#[derive(Serialize, Deserialize)]
pub struct ResetPasswordRequest {
    pub password: String,
}

#[derive(Serialize, Deserialize)]
pub struct TransferSlugRequest {
    pub slug: String,
    pub new_owner_user_id: i64,
}

#[derive(Serialize, Deserialize)]
pub struct ModerateSlugRequest {
    pub slug: String,
    pub action: String,   // 'flagged', 'disabled', 'active'
    pub severity: String, // 'low', 'medium', 'high', 'critical'
    pub reason: String,
}

#[derive(Serialize)]
pub struct ModerationEventResponse {
    pub id: String,
    pub timestamp: String,
    pub admin_username: String,
    pub target_user_id: i64,
    pub target_username: Option<String>,
    pub resource_type: String,
    pub resource_identifier: String,
    pub action: String,
    pub severity: String,
    pub reason: String,
}

#[derive(Serialize, Deserialize)]
pub struct ChangeOwnPasswordRequest {
    pub old_password: String,
    pub new_password: String,
}

#[derive(Serialize, Deserialize)]
pub struct CreateApiTokenRequest {
    // No request body needed, token is generated securely
}

#[derive(Serialize)]
pub struct CreateApiTokenResponse {
    pub id: i64,
    pub token: String, // Cleartext token returned once
    pub created_at: String,
}

// Helper: Ensure the request actor is an Admin
#[allow(clippy::result_large_err)]
fn require_admin_role(user: &ApiUser) -> Result<&crate::models::User, Response> {
    match &user.0 {
        ApiActor::Admin(admin) => Ok(admin),
        _ => Err((StatusCode::FORBIDDEN, "Admin privileges required").into_response()),
    }
}

// --- Admin: User CRUD Endpoints ---

// GET /api/v1/admin/users
pub async fn admin_list_users(State(state): State<AppState>, user: ApiUser) -> Response {
    if let Err(err_resp) = require_admin_role(&user) {
        return err_resp;
    }

    let conn = state.users_db.lock().unwrap();
    match crate::db::users::list_users(&conn) {
        Ok(users) => {
            let resp: Vec<UserResponse> = users
                .into_iter()
                .map(|u| UserResponse {
                    id: u.id,
                    username: u.username,
                    status: u.status,
                    account_type: u.account_type,
                    created_at: u.created_at,
                    metadata: u.metadata,
                })
                .collect();
            Json(resp).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// POST /api/v1/admin/users
pub async fn admin_create_user(
    State(state): State<AppState>,
    user: ApiUser,
    Json(payload): Json<CreateUserRequest>,
) -> Response {
    let admin = match require_admin_role(&user) {
        Ok(a) => a,
        Err(err_resp) => return err_resp,
    };

    // Username validation: minimum 3 chars, alphanumeric, hyphen, underscore
    let username = payload.username.trim().to_lowercase();
    if username.len() < 3 {
        return (
            StatusCode::BAD_REQUEST,
            "Username must be at least 3 characters",
        )
            .into_response();
    }
    if !username
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return (
            StatusCode::BAD_REQUEST,
            "Username must contain only alphanumeric characters, hyphens, or underscores",
        )
            .into_response();
    }

    // Hash password
    let hash = match crate::auth::password::hash_password(&payload.password) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Hashing error: {}", e),
            )
                .into_response()
        }
    };

    let conn = state.users_db.lock().unwrap();
    let account_type = payload.account_type.as_deref().unwrap_or("standard");
    match crate::db::users::create_user(
        &conn,
        &username,
        &hash,
        account_type,
        payload.metadata.as_deref(),
    ) {
        Ok(new_user) => {
            // Write system audit event
            {
                let system_conn = state.system_db.lock().unwrap();
                let _ = crate::db::audit_events::write_audit_event(
                    &system_conn,
                    &admin.username,
                    "USER_CREATION",
                    "user",
                    &new_user.id.to_string(),
                    Some(&format!("Username: {}", new_user.username)),
                );
            }

            Json(UserResponse {
                id: new_user.id,
                username: new_user.username,
                status: new_user.status,
                account_type: new_user.account_type,
                created_at: new_user.created_at,
                metadata: new_user.metadata,
            })
            .into_response()
        }
        Err(rusqlite::Error::SqliteFailure(err, _))
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            (StatusCode::CONFLICT, "Username already exists").into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// PUT /api/v1/admin/users/:id/status
pub async fn admin_update_user_status(
    State(state): State<AppState>,
    user: ApiUser,
    Path(target_id): Path<i64>,
    Json(payload): Json<UpdateUserStatusRequest>,
) -> Response {
    let admin = match require_admin_role(&user) {
        Ok(a) => a,
        Err(err_resp) => return err_resp,
    };

    let status = payload.status.trim().to_lowercase();
    if !["active", "disabled", "suspended", "pending", "deleted"].contains(&status.as_str()) {
        return (StatusCode::BAD_REQUEST, "Invalid user status").into_response();
    }

    let conn = state.users_db.lock().unwrap();
    match crate::db::users::update_user_status(&conn, target_id, &status) {
        Ok(_) => {
            let system_conn = state.system_db.lock().unwrap();
            let _ = crate::db::audit_events::write_audit_event(
                &system_conn,
                &admin.username,
                "USER_STATUS_UPDATE",
                "user",
                &target_id.to_string(),
                Some(&format!("New Status: {}", status)),
            );
            StatusCode::OK.into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// PUT /api/v1/admin/users/:id/quotas
pub async fn admin_update_user_quotas(
    State(state): State<AppState>,
    user: ApiUser,
    Path(target_id): Path<i64>,
    Json(payload): Json<UpdateUserQuotasRequest>,
) -> Response {
    let admin = match require_admin_role(&user) {
        Ok(a) => a,
        Err(err_resp) => return err_resp,
    };

    let conn = state.users_db.lock().unwrap();
    match crate::db::users::update_user_quotas(
        &conn,
        target_id,
        payload.max_urls,
        payload.max_landings,
        payload.max_api_tokens,
        payload.max_storage_mb,
    ) {
        Ok(_) => {
            let system_conn = state.system_db.lock().unwrap();
            let _ = crate::db::audit_events::write_audit_event(
                &system_conn,
                &admin.username,
                "USER_QUOTA_UPDATE",
                "user",
                &target_id.to_string(),
                Some(&format!(
                    "max_urls: {}, max_landings: {}, max_api_tokens: {}, max_storage_mb: {}",
                    payload.max_urls,
                    payload.max_landings,
                    payload.max_api_tokens,
                    payload.max_storage_mb
                )),
            );
            StatusCode::OK.into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// POST /api/v1/admin/users/:id/password
pub async fn admin_reset_user_password(
    State(state): State<AppState>,
    user: ApiUser,
    Path(target_id): Path<i64>,
    Json(payload): Json<ResetPasswordRequest>,
) -> Response {
    let admin = match require_admin_role(&user) {
        Ok(a) => a,
        Err(err_resp) => return err_resp,
    };

    let hash = match crate::auth::password::hash_password(&payload.password) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Hashing error: {}", e),
            )
                .into_response()
        }
    };

    let conn = state.users_db.lock().unwrap();
    match crate::db::users::reset_user_password(&conn, target_id, &hash) {
        Ok(_) => {
            let system_conn = state.system_db.lock().unwrap();
            let _ = crate::db::audit_events::write_audit_event(
                &system_conn,
                &admin.username,
                "USER_PASSWORD_RESET",
                "user",
                &target_id.to_string(),
                None,
            );
            StatusCode::OK.into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub fn delete_user_resources(
    state: &AppState,
    target_id: i64,
    admin_username: &str,
    force: bool,
) -> Result<(), String> {
    if target_id == 1 && !force {
        return Err("Deleting legacy_admin system account requires force flag".to_string());
    }

    let user_details = {
        let conn = state.users_db.lock().unwrap();
        match crate::db::users::get_user_by_id(&conn, target_id) {
            Ok(Some(u)) => u,
            Ok(None) => return Err("User not found".to_string()),
            Err(e) => return Err(e.to_string()),
        }
    };

    // 1. Transactional clean up on v0.8 slug databases
    let now = Utc::now().to_rfc3339();
    if let Some(ref tid) = user_details.tenant_id {
        let tid_str = tid.to_string();
        let urls_conn = state.db.global_urls.lock().unwrap();
        let pages_conn = state.db.global_landing_pages.lock().unwrap();
        let system_conn = state.system_db.lock().unwrap();

        // Get all URL slugs owned by tenant
        let url_slugs: Vec<String> = if let Ok(mut stmt) =
            urls_conn.prepare("SELECT slug FROM global_urls WHERE owner_tenant_id = ?1;")
        {
            stmt.query_map([&tid_str], |row| row.get::<_, String>(0))
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        for slug in url_slugs {
            let _ = urls_conn.execute("DELETE FROM global_urls WHERE slug = ?1;", [&slug]);
            let _ = system_conn.execute(
                "INSERT INTO slug_history (slug, old_owner_user_id, new_owner_user_id, action, timestamp, admin_username)
                 VALUES (?1, ?2, NULL, 'deleted', ?3, ?4);",
                rusqlite::params![slug, target_id, now, admin_username],
            );
        }

        // Get all page slugs owned by tenant
        let page_slugs: Vec<String> = if let Ok(mut stmt) =
            pages_conn.prepare("SELECT slug FROM global_landing_pages WHERE owner_tenant_id = ?1;")
        {
            stmt.query_map([&tid_str], |row| row.get::<_, String>(0))
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        for slug in page_slugs {
            let _ =
                pages_conn.execute("DELETE FROM global_landing_pages WHERE slug = ?1;", [&slug]);
            let _ = system_conn.execute(
                "INSERT INTO slug_history (slug, old_owner_user_id, new_owner_user_id, action, timestamp, admin_username)
                 VALUES (?1, ?2, NULL, 'deleted', ?3, ?4);",
                rusqlite::params![slug, target_id, now, admin_username],
            );
        }
    }

    let user_dir = {
        let conn = state.users_db.lock().unwrap();
        match crate::db::users::get_user_by_id(&conn, target_id) {
            Ok(Some(u)) => crate::db::tenant::location_for_user(&u)
                .and_then(|loc| {
                    loc.dir(&state.db.topology)
                        .map_err(|e| crate::error::AppError::BadRequest(e.to_string()))
                })
                .map_err(|e| e.to_string())?,
            _ => return Err("User not found".into()),
        }
    };
    if user_dir.exists() {
        let _ = std::fs::remove_dir_all(&user_dir);
    }

    let conn = state.users_db.lock().unwrap();
    crate::db::users::delete_user(&conn, target_id).map_err(|e| e.to_string())?;

    let system_conn = state.system_db.lock().unwrap();
    let _ = crate::db::audit_events::write_audit_event(
        &system_conn,
        admin_username,
        "USER_DELETION",
        "user",
        &target_id.to_string(),
        Some(&format!("Username: {}", user_details.username)),
    );

    Ok(())
}

// DELETE /api/v1/admin/users/:id
pub async fn admin_delete_user(
    State(state): State<AppState>,
    user: ApiUser,
    Path(target_id): Path<i64>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let admin = match require_admin_role(&user) {
        Ok(a) => a,
        Err(err_resp) => return err_resp,
    };

    let force = params.get("force").map(|v| v == "true").unwrap_or(false);
    match delete_user_resources(&state, target_id, &admin.username, force) {
        Ok(_) => StatusCode::OK.into_response(),
        Err(err) if err == "User not found" => StatusCode::NOT_FOUND.into_response(),
        Err(err) if err == "Deleting legacy_admin system account requires force flag" => {
            (StatusCode::BAD_REQUEST, err).into_response()
        }
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err).into_response(),
    }
}

// --- Admin: Slug Transfer ---

// POST /api/v1/admin/transfers
pub async fn admin_transfer_slug(
    State(state): State<AppState>,
    user: ApiUser,
    Json(payload): Json<TransferSlugRequest>,
) -> Response {
    let admin = match require_admin_role(&user) {
        Ok(a) => a,
        Err(err_resp) => return err_resp,
    };

    let req = crate::services::slug_transfer::SlugTransferRequest {
        slug: payload.slug,
        new_owner_user_id: payload.new_owner_user_id,
    };

    match crate::services::slug_transfer::transfer_slug(&state, &req, &admin.username) {
        Ok(_) => StatusCode::OK.into_response(),
        Err(crate::services::slug_transfer::TransferError::NotFound(m)) => {
            (StatusCode::NOT_FOUND, m.to_string()).into_response()
        }
        Err(crate::services::slug_transfer::TransferError::BadRequest(m)) => {
            (StatusCode::BAD_REQUEST, m).into_response()
        }
        Err(crate::services::slug_transfer::TransferError::Internal(m)) => {
            (StatusCode::INTERNAL_SERVER_ERROR, m).into_response()
        }
    }
}

// --- Admin: Content Moderation ---

// POST /api/v1/admin/moderation
pub async fn admin_moderate_slug(
    State(state): State<AppState>,
    user: ApiUser,
    Json(payload): Json<ModerateSlugRequest>,
) -> Response {
    let admin = match require_admin_role(&user) {
        Ok(a) => a,
        Err(err_resp) => return err_resp,
    };

    let action = payload.action.trim().to_lowercase();
    if !["flagged", "disabled", "active"].contains(&action.as_str()) {
        return (StatusCode::BAD_REQUEST, "Invalid moderation action").into_response();
    }

    // 1. Verify slug using v0.8 slug lookup
    let slug_info = match state.lookup_slug(&payload.slug) {
        Ok(Some(info)) => info,
        Ok(None) => return (StatusCode::NOT_FOUND, "Slug not found").into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let target_type = slug_info.target_type.as_str().to_string();
    let owner_tenant_id = match TenantId::parse(&slug_info.owner_tenant_id) {
        Ok(tid) => tid,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Invalid owner tenant ID on slug",
            )
                .into_response();
        }
    };

    // Resolve owner details from users.db for moderation event history
    let (owner_user_id, owner_username) = {
        let users_conn = state.users_db.lock().unwrap();
        match crate::db::users::get_user_by_tenant_id(&users_conn, owner_tenant_id) {
            Ok(Some(u)) => (u.id, Some(u.username)),
            _ => (0, None),
        }
    };

    // 2. Perform moderation update in authoritative v0.8 slug databases
    {
        let urls_conn = state.db.global_urls.lock().unwrap();
        let pages_conn = state.db.global_landing_pages.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        if slug_info.target_type == crate::db::slugs::SlugTargetType::Url {
            let _ = urls_conn.execute(
                "UPDATE global_urls SET status = ?1, updated_at = ?2 WHERE slug = ?3;",
                rusqlite::params![action, now, payload.slug],
            );
        } else {
            let _ = pages_conn.execute(
                "UPDATE global_landing_pages SET status = ?1, updated_at = ?2 WHERE slug = ?3;",
                rusqlite::params![action, now, payload.slug],
            );
        }
    }

    // 3. Sync status to owner tenant's content DB if possible
    if let Ok(tenant_dbs) =
        state.open_tenant(owner_tenant_id, crate::db::tenant::TenantOpenMode::CoreJob)
    {
        if let Ok(conn) = tenant_dbs.content.lock() {
            if slug_info.target_type == crate::db::slugs::SlugTargetType::Url {
                let content_status = if action == "disabled" {
                    "dead"
                } else {
                    "active"
                };
                let _ = conn.execute(
                    "UPDATE urls SET status = ?1 WHERE code = ?2;",
                    rusqlite::params![content_status, payload.slug],
                );
            } else {
                let content_state = if action == "disabled" {
                    "archived"
                } else {
                    "published"
                };
                let _ = conn.execute(
                    "UPDATE landing_pages SET state = ?1 WHERE code = ?2;",
                    rusqlite::params![content_state, payload.slug],
                );
            }
        }
    }

    // 4. Record moderation event in system.db
    {
        let system_conn = state.system_db.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let event_id = Uuid::new_v4().to_string();

        let _ = system_conn.execute(
            "INSERT INTO moderation_events (id, timestamp, admin_username, target_user_id, target_username, resource_type, resource_identifier, action, severity, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10);",
            rusqlite::params![
                event_id,
                now,
                admin.username,
                owner_user_id,
                owner_username,
                target_type,
                payload.slug,
                action,
                payload.severity,
                payload.reason
            ],
        );

        let _ = crate::db::audit_events::write_audit_event(
            &system_conn,
            &admin.username,
            "CONTENT_MODERATION",
            "slug",
            &payload.slug,
            Some(&format!("Action: {}, Reason: {}", action, payload.reason)),
        );
    }

    StatusCode::OK.into_response()
}

// GET /api/v1/admin/moderation/events
pub async fn admin_list_moderation_events(
    State(state): State<AppState>,
    user: ApiUser,
) -> Response {
    if let Err(err_resp) = require_admin_role(&user) {
        return err_resp;
    }

    let system_conn = state.system_db.lock().unwrap();
    let mut stmt = match system_conn.prepare(
        "SELECT id, timestamp, admin_username, target_user_id, target_username, resource_type, resource_identifier, action, severity, reason 
         FROM moderation_events ORDER BY timestamp DESC;"
    ) {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let rows = stmt.query_map([], |row| {
        Ok(ModerationEventResponse {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            admin_username: row.get(2)?,
            target_user_id: row.get(3)?,
            target_username: row.get(4)?,
            resource_type: row.get(5)?,
            resource_identifier: row.get(6)?,
            action: row.get(7)?,
            severity: row.get(8)?,
            reason: row.get(9)?,
        })
    });

    match rows {
        Ok(mapped) => {
            let events: Vec<ModerationEventResponse> = mapped.filter_map(|r| r.ok()).collect();
            Json(events).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// --- User: Dashboard, profile settings, API tokens ---

// GET /api/v1/user/profile
pub async fn user_get_profile(State(state): State<AppState>, user: ApiUser) -> Response {
    let tenant_user = match user.0 {
        ApiActor::User(u) => u,
        ApiActor::Admin(_) => {
            return (
                StatusCode::BAD_REQUEST,
                "Profile endpoints are for tenant users only",
            )
                .into_response();
        }
    };

    let users_conn = state.users_db.lock().unwrap();
    let quotas = crate::db::users::get_user_quotas(&users_conn, tenant_user.id).unwrap_or(None);

    Json(serde_json::json!({
        "id": tenant_user.id,
        "username": tenant_user.username,
        "status": tenant_user.status,
        "account_type": tenant_user.account_type,
        "created_at": tenant_user.created_at,
        "metadata": tenant_user.metadata,
        "quotas": quotas,
    }))
    .into_response()
}

// POST /api/v1/user/password
pub async fn user_change_password(
    State(state): State<AppState>,
    user: ApiUser,
    Json(payload): Json<ChangeOwnPasswordRequest>,
) -> Response {
    let tenant_user = match user.0 {
        ApiActor::User(u) => u,
        ApiActor::Admin(_) => {
            return (
                StatusCode::BAD_REQUEST,
                "Change password is for tenant users only",
            )
                .into_response();
        }
    };

    // Verify old password
    if !crate::auth::password::verify_password(&payload.old_password, &tenant_user.password_hash) {
        return (StatusCode::UNAUTHORIZED, "Invalid current password").into_response();
    }

    // Hash new password
    let hash = match crate::auth::password::hash_password(&payload.new_password) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Hashing error: {}", e),
            )
                .into_response()
        }
    };

    let users_conn = state.users_db.lock().unwrap();
    match crate::db::users::reset_user_password(&users_conn, tenant_user.id, &hash) {
        Ok(_) => {
            let system_conn = state.system_db.lock().unwrap();
            let _ = crate::db::audit_events::write_audit_event(
                &system_conn,
                &tenant_user.username,
                "PASSWORD_CHANGE",
                "user",
                &tenant_user.id.to_string(),
                None,
            );
            StatusCode::OK.into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// GET /api/v1/user/api-tokens
pub async fn user_list_api_tokens(State(state): State<AppState>, user: ApiUser) -> Response {
    let tenant_user = match user.0 {
        ApiActor::User(u) => u,
        ApiActor::Admin(_) => {
            return (
                StatusCode::BAD_REQUEST,
                "API tokens are for tenant users only",
            )
                .into_response();
        }
    };

    let users_conn = state.users_db.lock().unwrap();
    match crate::db::users::list_user_api_tokens(&users_conn, tenant_user.id) {
        Ok(tokens) => Json(tokens).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// POST /api/v1/user/api-tokens
pub async fn user_create_api_token(State(state): State<AppState>, user: ApiUser) -> Response {
    let tenant_user = match user.0 {
        ApiActor::User(u) => u,
        ApiActor::Admin(_) => {
            return (
                StatusCode::BAD_REQUEST,
                "API tokens are for tenant users only",
            )
                .into_response();
        }
    };

    // 1. Quota check
    let users_conn = state.users_db.lock().unwrap();
    let quotas = crate::db::users::get_user_quotas(&users_conn, tenant_user.id).unwrap_or(None);
    if let Some(quota) = quotas {
        if quota.current_api_tokens >= quota.max_api_tokens {
            return (StatusCode::BAD_REQUEST, "API tokens quota limit exceeded").into_response();
        }
    }

    // 2. Generate secure token
    let token_secret = format!("bzo_{}", crate::auth::session::generate_token(16)); // bzo_ followed by 32 hex chars

    // Hash token using SHA-256 for storing
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(token_secret.as_bytes());
    let token_hash = hex::encode(hasher.finalize());

    match crate::db::users::create_user_api_token(&users_conn, tenant_user.id, &token_hash) {
        Ok(api_token) => {
            let system_conn = state.system_db.lock().unwrap();
            let _ = crate::db::audit_events::write_audit_event(
                &system_conn,
                &tenant_user.username,
                "API_TOKEN_CREATION",
                "api_token",
                &api_token.id.to_string(),
                None,
            );

            Json(CreateApiTokenResponse {
                id: api_token.id,
                token: token_secret, // Return cleartext once
                created_at: api_token.created_at,
            })
            .into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// DELETE /api/v1/user/api-tokens/:id
pub async fn user_delete_api_token(
    State(state): State<AppState>,
    user: ApiUser,
    Path(token_id): Path<i64>,
) -> Response {
    let tenant_user = match user.0 {
        ApiActor::User(u) => u,
        ApiActor::Admin(_) => {
            return (
                StatusCode::BAD_REQUEST,
                "API tokens are for tenant users only",
            )
                .into_response();
        }
    };

    let users_conn = state.users_db.lock().unwrap();
    match crate::db::users::delete_user_api_token(&users_conn, token_id, tenant_user.id) {
        Ok(_) => {
            let system_conn = state.system_db.lock().unwrap();
            let _ = crate::db::audit_events::write_audit_event(
                &system_conn,
                &tenant_user.username,
                "API_TOKEN_DELETION",
                "api_token",
                &token_id.to_string(),
                None,
            );
            StatusCode::OK.into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
