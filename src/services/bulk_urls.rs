//! Bulk URL creation business logic (transaction + slug reservation).
//!
//! Handlers own auth/HTTP; this module owns validation, reservation, and inserts.

use crate::auth::generate_token;
use crate::auth::password::hash_password;
use crate::identity::TenantId;
use crate::models::Url;
use crate::utils::validation::validate_redirect_destination;
use rusqlite::{Connection, Transaction};
use std::sync::Mutex;

/// One item in a bulk URL create request (mirrors the HTTP payload shape).
#[derive(Debug, Clone)]
pub struct BulkUrlCreateItem {
    pub destination: String,
    pub code: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub expires_at: Option<String>,
    pub password: Option<String>,
    pub max_access_count: Option<i64>,
}

#[derive(Debug)]
pub enum BulkUrlError {
    BadRequest(String),
    Conflict(String),
    Forbidden(String),
    Internal(String),
}

impl BulkUrlError {
    pub fn message(&self) -> &str {
        match self {
            Self::BadRequest(m) | Self::Conflict(m) | Self::Forbidden(m) | Self::Internal(m) => m,
        }
    }
}

fn release_reserved(urls_conn: &Connection, slugs: &[String], owner_tenant_id: &TenantId) {
    for slug in slugs {
        let _ = crate::db::slugs::release_url_slug(urls_conn, slug, owner_tenant_id);
    }
}

/// Check that the tenant can accept `additional` new URLs.
pub fn ensure_url_quota(
    users_db: &Mutex<Connection>,
    user_id: i64,
    additional: i64,
) -> Result<(), BulkUrlError> {
    let users_conn = crate::utils::lock_db(users_db, "users_db")
        .map_err(|e| BulkUrlError::Internal(e.to_string()))?;
    match crate::db::users::get_user_quotas(&users_conn, user_id) {
        Ok(Some(quotas)) => {
            if quotas.current_urls + additional > quotas.max_urls {
                Err(BulkUrlError::Forbidden("Quota limit exceeded".into()))
            } else {
                Ok(())
            }
        }
        Ok(None) => Err(BulkUrlError::Forbidden("User quota not found".into())),
        Err(e) => Err(BulkUrlError::Internal(format!("quota lookup failed: {e}"))),
    }
}

/// Create many URLs inside a single content transaction with global slug reservation.
#[allow(clippy::too_many_arguments)]
pub fn create_urls_bulk(
    content_db: &Mutex<Connection>,
    reserved_db: &Mutex<Connection>,
    global_urls_db: &Mutex<Connection>,
    global_landing_pages_db: &Mutex<Connection>,
    users_db: &Mutex<Connection>,
    owner_user_id: i64,
    owner_tenant_id: TenantId,
    items: Vec<BulkUrlCreateItem>,
) -> Result<Vec<Url>, BulkUrlError> {
    let mut conn = crate::utils::lock_db(content_db, "content_db")
        .map_err(|e| BulkUrlError::Internal(e.to_string()))?;
    let tx = conn.transaction().map_err(|e| {
        BulkUrlError::Internal(format!("Failed to start database transaction: {e}"))
    })?;

    let mut created_urls = Vec::new();
    let mut reserved_slugs: Vec<String> = Vec::new();

    for item in items {
        match create_one_in_tx(
            &tx,
            reserved_db,
            global_urls_db,
            global_landing_pages_db,
            &owner_tenant_id,
            item,
            &mut reserved_slugs,
        ) {
            Ok(url) => created_urls.push(url),
            Err(e) => {
                let _ = tx.rollback();
                if let Ok(urls_conn) = crate::utils::lock_db(global_urls_db, "global_urls_db") {
                    release_reserved(&urls_conn, &reserved_slugs, &owner_tenant_id);
                }
                return Err(e);
            }
        }
    }

    if let Err(e) = tx.commit() {
        if let Ok(urls_conn) = crate::utils::lock_db(global_urls_db, "global_urls_db") {
            release_reserved(&urls_conn, &reserved_slugs, &owner_tenant_id);
        }
        return Err(BulkUrlError::Internal(format!(
            "Failed to commit transaction: {e}"
        )));
    }

    // Activate slugs in v0.8 global_urls.db
    {
        let urls_conn = crate::utils::lock_db(global_urls_db, "global_urls_db")
            .map_err(|e| BulkUrlError::Internal(e.to_string()))?;
        for url in &created_urls {
            let _ = crate::db::slugs::activate_url_slug(&urls_conn, &url.code, &url.id);
        }
    }

    // Increment quota counters
    {
        let users_conn = crate::utils::lock_db(users_db, "users_db")
            .map_err(|e| BulkUrlError::Internal(e.to_string()))?;
        for _ in 0..created_urls.len() {
            let _ = crate::db::users::increment_quota_counter(&users_conn, owner_user_id, "urls");
        }
    }

    Ok(created_urls)
}

fn create_one_in_tx(
    tx: &Transaction<'_>,
    reserved_db: &Mutex<Connection>,
    global_urls_db: &Mutex<Connection>,
    global_landing_pages_db: &Mutex<Connection>,
    owner_tenant_id: &TenantId,
    item: BulkUrlCreateItem,
    reserved_slugs: &mut Vec<String>,
) -> Result<Url, BulkUrlError> {
    let mut code = item.code.unwrap_or_default().trim().to_lowercase();
    if code.is_empty() {
        code = generate_token(3);
    } else if !crate::utils::validation::validate_redirect_code(&code) {
        return Err(BulkUrlError::BadRequest(format!(
            "Short code or slug '{code}' is invalid (must be 6 hex characters or !custom-slug)"
        )));
    }

    {
        let reserved_conn = crate::utils::lock_db(reserved_db, "reserved_db")
            .map_err(|e| BulkUrlError::Internal(e.to_string()))?;
        let urls_conn = crate::utils::lock_db(global_urls_db, "global_urls_db")
            .map_err(|e| BulkUrlError::Internal(e.to_string()))?;
        let pages_conn = crate::utils::lock_db(global_landing_pages_db, "global_landing_pages_db")
            .map_err(|e| BulkUrlError::Internal(e.to_string()))?;

        let available =
            crate::db::slugs::is_slug_available(&reserved_conn, &urls_conn, &pages_conn, &code)
                .unwrap_or(false)
                && !reserved_slugs.contains(&code);

        if !available {
            return Err(BulkUrlError::Conflict(format!(
                "Short code '{code}' already exists"
            )));
        }

        if let Err(e) = crate::db::slugs::reserve_url_slug(
            &reserved_conn,
            &urls_conn,
            &pages_conn,
            &code,
            owner_tenant_id,
        ) {
            return Err(BulkUrlError::Internal(format!(
                "Failed to reserve slug '{code}': {e}"
            )));
        }
        reserved_slugs.push(code.clone());
    }

    let password_hash = if let Some(ref pwd) = item.password {
        match hash_password(pwd) {
            Ok(h) => Some(h),
            Err(e) => {
                return Err(BulkUrlError::Internal(format!(
                    "Password hashing error: {e}"
                )));
            }
        }
    } else {
        None
    };

    if !validate_redirect_destination(&item.destination) {
        return Err(BulkUrlError::BadRequest(format!(
            "Invalid destination for item '{code}': must be a valid http(s) URL without control characters"
        )));
    }

    let tags = item.tags.unwrap_or_default();
    crate::db::content::create_url_extended(
        tx,
        &code,
        &item.destination,
        item.title.as_deref(),
        item.description.as_deref(),
        &tags,
        item.expires_at.as_deref(),
        password_hash.as_deref(),
        item.max_access_count,
    )
    .map_err(|e| BulkUrlError::Internal(format!("Database insert error: {e}")))
}
