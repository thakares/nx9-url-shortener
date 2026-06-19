use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use chrono::Utc;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::ApiUser;
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

    // 1. Transactional clean up on system.db (deleting their global slug mappings)
    {
        let mut system_conn = state.system_db.lock().unwrap();
        let tx = system_conn.transaction().map_err(|e| e.to_string())?;

        let slugs: Vec<String> = {
            let mut stmt = tx
                .prepare("SELECT slug FROM global_slugs WHERE owner_user_id = ?1;")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([target_id], |row| row.get(0))
                .map_err(|e| e.to_string())?;
            rows.filter_map(|r| r.ok()).collect()
        };

        let now = Utc::now().to_rfc3339();
        for slug in slugs {
            let _ = tx.execute("DELETE FROM global_slugs WHERE slug = ?1;", [&slug]);
            let _ = tx.execute(
                "INSERT INTO slug_history (slug, old_owner_user_id, new_owner_user_id, action, timestamp, admin_username)
                 VALUES (?1, ?2, NULL, 'deleted', ?3, ?4);",
                rusqlite::params![slug, target_id, now, admin_username],
            );
        }

        tx.commit()
            .map_err(|e| format!("Failed to release slugs: {}", e))?;
    }

    let user_dir = state
        .config
        .data_dir
        .join("users")
        .join(target_id.to_string());
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

    // 1. Check if the slug exists and get details
    let (old_owner_user_id, target_type, _target_id) = {
        let system_conn = state.system_db.lock().unwrap();
        let mut stmt = match system_conn.prepare(
            "SELECT owner_user_id, target_type, target_id FROM global_slugs WHERE slug = ?1;",
        ) {
            Ok(s) => s,
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        };
        let row_opt = stmt
            .query_row([&payload.slug], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .optional();

        match row_opt {
            Ok(Some(r)) => r,
            Ok(None) => return (StatusCode::NOT_FOUND, "Slug not found").into_response(),
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        }
    };

    if old_owner_user_id == payload.new_owner_user_id {
        return (
            StatusCode::BAD_REQUEST,
            "New owner must be different from the current owner",
        )
            .into_response();
    }

    // 2. Fetch destination databases
    let old_dbs = match state.get_user_dbs(old_owner_user_id) {
        Ok(dbs) => dbs,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to load current owner's database",
            )
                .into_response()
        }
    };
    let new_dbs = match state.get_user_dbs(payload.new_owner_user_id) {
        Ok(dbs) => dbs,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to load new owner's database",
            )
                .into_response()
        }
    };

    // 3. Perform transfer: copy record from old owner's content.db to new owner's content.db
    let mut new_target_id = String::new();
    let transfer_success = {
        let old_conn = old_dbs.content.lock().unwrap();
        let new_conn = new_dbs.content.lock().unwrap();

        if target_type == "url" {
            // Get URL record
            let url_opt = match crate::db::content::get_url_by_code(&old_conn, &payload.slug) {
                Ok(u) => u,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
                }
            };

            if let Some(url) = url_opt {
                // Check new owner quotas
                let new_users_conn = state.users_db.lock().unwrap();
                let quota_opt =
                    crate::db::users::get_user_quotas(&new_users_conn, payload.new_owner_user_id)
                        .unwrap_or(None);
                if let Some(quota) = quota_opt {
                    if quota.current_urls >= quota.max_urls {
                        return (
                            StatusCode::BAD_REQUEST,
                            "New owner has exceeded URL quota limit",
                        )
                            .into_response();
                    }
                }

                // Insert into new owner database
                let ins_res = crate::db::content::create_url_extended(
                    &new_conn,
                    &url.code,
                    &url.destination,
                    url.title.as_deref(),
                    url.description.as_deref(),
                    &url.tags,
                    url.expires_at.as_deref(),
                    url.password_hash.as_deref(),
                    url.max_access_count,
                );

                match ins_res {
                    Ok(new_url) => {
                        new_target_id = new_url.id;
                        // Delete from old owner database
                        let _ = crate::db::content::delete_url(&old_conn, &url.id);
                        true
                    }
                    Err(e) => {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("Failed to copy URL to new owner: {}", e),
                        )
                            .into_response();
                    }
                }
            } else {
                false
            }
        } else if target_type == "page" {
            // Get page record
            let page_opt =
                match crate::db::content::get_landing_page_by_code(&old_conn, &payload.slug) {
                    Ok(p) => p,
                    Err(e) => {
                        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
                    }
                };

            if let Some(page) = page_opt {
                // Check new owner quotas
                let new_users_conn = state.users_db.lock().unwrap();
                let quota_opt =
                    crate::db::users::get_user_quotas(&new_users_conn, payload.new_owner_user_id)
                        .unwrap_or(None);
                if let Some(quota) = quota_opt {
                    if quota.current_landings >= quota.max_landings {
                        return (
                            StatusCode::BAD_REQUEST,
                            "New owner has exceeded landing page quota limit",
                        )
                            .into_response();
                    }
                }

                // Insert into new owner database
                let ins_res = crate::db::content::create_landing_page(
                    &new_conn,
                    &page.code,
                    &page.slug,
                    &page.title,
                    &page.html_content,
                    &page.state,
                );

                match ins_res {
                    Ok(new_page) => {
                        new_target_id = new_page.id;
                        // Delete from old owner database
                        let _ = crate::db::content::delete_landing_page(&old_conn, &page.id);
                        true
                    }
                    Err(e) => {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("Failed to copy Page to new owner: {}", e),
                        )
                            .into_response();
                    }
                }
            } else {
                false
            }
        } else {
            false
        }
    };

    if !transfer_success {
        return (StatusCode::NOT_FOUND, "Content not found in owner database").into_response();
    }

    // 4. Update system global_slugs, slug_history and adjust quotas
    {
        let system_conn = state.system_db.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        let _ = system_conn.execute(
            "UPDATE global_slugs SET owner_user_id = ?1, target_id = ?2, updated_at = ?3 WHERE slug = ?4;",
            rusqlite::params![payload.new_owner_user_id, new_target_id, now, payload.slug],
        );

        let _ = system_conn.execute(
            "INSERT INTO slug_history (slug, old_owner_user_id, new_owner_user_id, action, timestamp, admin_username)
             VALUES (?1, ?2, ?3, 'transferred', ?4, ?5);",
            rusqlite::params![payload.slug, old_owner_user_id, payload.new_owner_user_id, now, admin.username],
        );

        // Adjust quotas
        let users_conn = state.users_db.lock().unwrap();
        let field = if target_type == "url" {
            "urls"
        } else {
            "landings"
        };
        let _ = crate::db::users::decrement_quota_counter(&users_conn, old_owner_user_id, field);
        let _ = crate::db::users::increment_quota_counter(
            &users_conn,
            payload.new_owner_user_id,
            field,
        );

        let _ = crate::db::audit_events::write_audit_event(
            &system_conn,
            &admin.username,
            "SLUG_TRANSFER",
            "slug",
            &payload.slug,
            Some(&format!(
                "From owner {} to owner {}",
                old_owner_user_id, payload.new_owner_user_id
            )),
        );
    }

    StatusCode::OK.into_response()
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

    // 1. Verify slug and get owner user ID
    let (owner_user_id, target_type) = {
        let system_conn = state.system_db.lock().unwrap();
        let row_opt: Option<(i64, String)> = system_conn
            .query_row(
                "SELECT owner_user_id, target_type FROM global_slugs WHERE slug = ?1;",
                [&payload.slug],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .unwrap_or(None);

        match row_opt {
            Some(r) => r,
            None => return (StatusCode::NOT_FOUND, "Slug not found").into_response(),
        }
    };

    // Resolve owner username for log snapshot
    let owner_username = {
        let users_conn = state.users_db.lock().unwrap();
        crate::db::users::get_user_by_id(&users_conn, owner_user_id)
            .unwrap_or(None)
            .map(|u| u.username)
    };

    // 2. Perform moderation update in global_slugs
    {
        let system_conn = state.system_db.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        let _ = system_conn.execute(
            "UPDATE global_slugs SET status = ?1, updated_at = ?2 WHERE slug = ?3;",
            rusqlite::params![action, now, payload.slug],
        );

        // Record moderation event
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
