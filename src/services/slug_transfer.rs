//! Cross-tenant slug transfer business logic.
//!
//! Copies URL/page content between tenant content DBs, then updates global_slugs
//! ownership. Handlers own admin auth and HTTP mapping.

use crate::state::{AppState, UserDbs};
use crate::utils::lock_db;
use chrono::Utc;
use rusqlite::OptionalExtension;

#[derive(Debug)]
pub enum TransferError {
    NotFound(&'static str),
    BadRequest(String),
    Internal(String),
}

impl TransferError {
    pub fn message(&self) -> String {
        match self {
            Self::NotFound(m) => (*m).to_string(),
            Self::BadRequest(m) | Self::Internal(m) => m.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SlugTransferRequest {
    pub slug: String,
    pub new_owner_user_id: i64,
}

#[derive(Debug)]
pub struct SlugTransferResult {
    pub old_owner_user_id: i64,
    pub new_owner_user_id: i64,
    pub target_type: String,
    pub new_target_id: String,
}

/// Look up slug ownership in `global_slugs`.
pub fn lookup_slug(state: &AppState, slug: &str) -> Result<(i64, String, String), TransferError> {
    let system_conn = lock_db(&state.system_db, "system_db")
        .map_err(|e| TransferError::Internal(e.to_string()))?;
    let mut stmt = system_conn
        .prepare("SELECT owner_user_id, target_type, target_id FROM global_slugs WHERE slug = ?1;")
        .map_err(|e| TransferError::Internal(e.to_string()))?;
    let row_opt = stmt
        .query_row([slug], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .optional()
        .map_err(|e| TransferError::Internal(e.to_string()))?;

    match row_opt {
        Some(r) => Ok(r),
        None => Err(TransferError::NotFound("Slug not found")),
    }
}

/// Copy content row between tenants and return the new target id.
fn copy_content(
    state: &AppState,
    old_dbs: &UserDbs,
    new_dbs: &UserDbs,
    slug: &str,
    target_type: &str,
    new_owner_user_id: i64,
) -> Result<String, TransferError> {
    let old_conn = lock_db(&old_dbs.content, "old_content_db")
        .map_err(|e| TransferError::Internal(e.to_string()))?;
    let new_conn = lock_db(&new_dbs.content, "new_content_db")
        .map_err(|e| TransferError::Internal(e.to_string()))?;

    if target_type == "url" {
        let url = match crate::db::content::get_url_by_code(&old_conn, slug) {
            Ok(Some(u)) => u,
            Ok(None) => {
                return Err(TransferError::NotFound(
                    "Content not found in owner database",
                ))
            }
            Err(e) => return Err(TransferError::Internal(e.to_string())),
        };

        {
            let new_users_conn = lock_db(&state.users_db, "users_db")
                .map_err(|e| TransferError::Internal(e.to_string()))?;
            if let Ok(Some(quota)) =
                crate::db::users::get_user_quotas(&new_users_conn, new_owner_user_id)
            {
                if quota.current_urls >= quota.max_urls {
                    return Err(TransferError::BadRequest(
                        "New owner has exceeded URL quota limit".into(),
                    ));
                }
            }
        }

        let new_url = crate::db::content::create_url_extended(
            &new_conn,
            &url.code,
            &url.destination,
            url.title.as_deref(),
            url.description.as_deref(),
            &url.tags,
            url.expires_at.as_deref(),
            url.password_hash.as_deref(),
            url.max_access_count,
        )
        .map_err(|e| TransferError::Internal(format!("Failed to copy URL to new owner: {e}")))?;
        let _ = crate::db::content::delete_url(&old_conn, &url.id);
        Ok(new_url.id)
    } else if target_type == "page" {
        let page = match crate::db::content::get_landing_page_by_code(&old_conn, slug) {
            Ok(Some(p)) => p,
            Ok(None) => {
                return Err(TransferError::NotFound(
                    "Content not found in owner database",
                ))
            }
            Err(e) => return Err(TransferError::Internal(e.to_string())),
        };

        {
            let new_users_conn = lock_db(&state.users_db, "users_db")
                .map_err(|e| TransferError::Internal(e.to_string()))?;
            if let Ok(Some(quota)) =
                crate::db::users::get_user_quotas(&new_users_conn, new_owner_user_id)
            {
                if quota.current_landings >= quota.max_landings {
                    return Err(TransferError::BadRequest(
                        "New owner has exceeded landing page quota limit".into(),
                    ));
                }
            }
        }

        let new_page = crate::db::content::create_landing_page(
            &new_conn,
            &page.code,
            &page.slug,
            &page.title,
            &page.html_content,
            &page.state,
        )
        .map_err(|e| TransferError::Internal(format!("Failed to copy Page to new owner: {e}")))?;
        let _ = crate::db::content::delete_landing_page(&old_conn, &page.id);
        Ok(new_page.id)
    } else {
        Err(TransferError::NotFound(
            "Content not found in owner database",
        ))
    }
}

/// Perform a full slug transfer (content + registry + quotas + history).
pub fn transfer_slug(
    state: &AppState,
    req: &SlugTransferRequest,
    admin_username: &str,
) -> Result<SlugTransferResult, TransferError> {
    let (old_owner_user_id, target_type, _target_id) = lookup_slug(state, &req.slug)?;

    if old_owner_user_id == req.new_owner_user_id {
        return Err(TransferError::BadRequest(
            "New owner must be different from the current owner".into(),
        ));
    }

    let old_dbs = state
        .get_user_dbs(old_owner_user_id)
        .map_err(|_| TransferError::Internal("Failed to load current owner's database".into()))?;
    let new_dbs = state
        .get_user_dbs(req.new_owner_user_id)
        .map_err(|_| TransferError::Internal("Failed to load new owner's database".into()))?;

    let new_target_id = copy_content(
        state,
        &old_dbs,
        &new_dbs,
        &req.slug,
        &target_type,
        req.new_owner_user_id,
    )?;

    {
        let system_conn = lock_db(&state.system_db, "system_db")
            .map_err(|e| TransferError::Internal(e.to_string()))?;
        let now = Utc::now().to_rfc3339();

        let _ = system_conn.execute(
            "UPDATE global_slugs SET owner_user_id = ?1, target_id = ?2, updated_at = ?3 WHERE slug = ?4;",
            rusqlite::params![req.new_owner_user_id, new_target_id, now, req.slug],
        );

        let _ = system_conn.execute(
            "INSERT INTO slug_history (slug, old_owner_user_id, new_owner_user_id, action, timestamp, admin_username)
             VALUES (?1, ?2, ?3, 'transferred', ?4, ?5);",
            rusqlite::params![
                req.slug,
                old_owner_user_id,
                req.new_owner_user_id,
                now,
                admin_username
            ],
        );

        let users_conn = lock_db(&state.users_db, "users_db")
            .map_err(|e| TransferError::Internal(e.to_string()))?;
        let field = if target_type == "url" {
            "urls"
        } else {
            "landings"
        };
        let _ = crate::db::users::decrement_quota_counter(&users_conn, old_owner_user_id, field);
        let _ =
            crate::db::users::increment_quota_counter(&users_conn, req.new_owner_user_id, field);

        let _ = crate::db::audit_events::write_audit_event(
            &system_conn,
            admin_username,
            "SLUG_TRANSFER",
            "slug",
            &req.slug,
            Some(&format!(
                "From owner {} to owner {}",
                old_owner_user_id, req.new_owner_user_id
            )),
        );
    }

    Ok(SlugTransferResult {
        old_owner_user_id,
        new_owner_user_id: req.new_owner_user_id,
        target_type,
        new_target_id,
    })
}
