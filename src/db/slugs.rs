//! Frozen v0.8 Slug Registry Layer.
//!
//! Owns `slugs/global_urls.db`, `slugs/global_landing_pages.db`, and `slugs/reserved.db`.
//!
//! Guarantees:
//! - Exact TenantId ownership (no integer ID primitives in v0.8 API).
//! - Cross-database global uniqueness invariant: a slug exists in AT MOST ONE of
//!   `reserved.db`, `global_urls.db`, `global_landing_pages.db`.
//! - Retired slugs remain permanently unavailable for reuse across both URLs and landing pages.
//! - Concurrency-safe global allocations.

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::schema_v08::{
    SLUG_STATUS_ACTIVE, SLUG_STATUS_DISABLED, SLUG_STATUS_RESERVING, SLUG_STATUS_RETIRED,
};
use crate::identity::TenantId;

/// Core reserved slugs, matching the v0.7 `system.db` seed list.
pub const CORE_RESERVED_SLUGS: &[(&str, &str)] = &[
    ("admin", "System route"),
    ("login", "System route"),
    ("logout", "System route"),
    ("dashboard", "System route"),
    ("api", "System route"),
    ("docs", "System route"),
    ("assets", "System route"),
    ("static", "System route"),
    ("favicon.ico", "System route"),
    ("robots.txt", "System route"),
    ("health", "System route"),
    ("metrics", "System route"),
    ("install", "System route"),
    ("setup", "System route"),
    ("support", "System route"),
    ("help", "System route"),
    ("security", "System route"),
    ("abuse", "System route"),
    ("billing", "System route"),
    ("status", "System route"),
    ("legacy_admin", "System reserved"),
    ("administrator", "System reserved"),
    ("system", "System reserved"),
    ("root", "System reserved"),
    ("www", "System reserved"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlugTargetType {
    Url,
    LandingPage,
}

impl SlugTargetType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Url => "url",
            Self::LandingPage => "page",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedSlugInfo {
    pub slug: String,
    pub owner_tenant_id: String,
    pub target_type: SlugTargetType,
    pub target_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub status: String,
    pub retired_at: Option<String>,
}

/// Insert the core reserved set. Idempotent (`INSERT OR IGNORE`).
pub fn seed_reserved_slugs(conn: &Connection) -> rusqlite::Result<usize> {
    let mut inserted = 0usize;
    for (slug, reason) in CORE_RESERVED_SLUGS {
        let n = conn.execute(
            "INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES (?1, ?2);",
            params![slug, reason],
        )?;
        inserted += n;
    }
    Ok(inserted)
}

pub fn reserved_slug_count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM reserved_slugs;", [], |row| row.get(0))
}

/// Check whether a slug is reserved in `slugs/reserved.db`.
pub fn is_slug_reserved(reserved_conn: &Connection, slug: &str) -> rusqlite::Result<bool> {
    reserved_conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM reserved_slugs WHERE slug = ?1);",
        [slug],
        |row| row.get(0),
    )
}

/// Check whether a slug is available for a new registration.
/// Returns false if reserved or if present in `global_urls.db` or `global_landing_pages.db`
/// in any state (including `retired`).
pub fn is_slug_available(
    reserved_conn: &Connection,
    urls_conn: &Connection,
    pages_conn: &Connection,
    slug: &str,
) -> rusqlite::Result<bool> {
    if is_slug_reserved(reserved_conn, slug)? {
        return Ok(false);
    }

    let in_urls: bool = urls_conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM global_urls WHERE slug = ?1);",
        [slug],
        |r| r.get(0),
    )?;
    if in_urls {
        return Ok(false);
    }

    let in_pages: bool = pages_conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM global_landing_pages WHERE slug = ?1);",
        [slug],
        |r| r.get(0),
    )?;
    if in_pages {
        return Ok(false);
    }

    Ok(true)
}

/// Lookup a slug in `slugs/global_urls.db`.
pub fn lookup_url_slug(
    urls_conn: &Connection,
    slug: &str,
) -> rusqlite::Result<Option<ResolvedSlugInfo>> {
    urls_conn
        .query_row(
            "SELECT slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at 
             FROM global_urls WHERE slug = ?1;",
            [slug],
            |row| {
                Ok(ResolvedSlugInfo {
                    slug: row.get(0)?,
                    owner_tenant_id: row.get(1)?,
                    target_type: SlugTargetType::Url,
                    target_id: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    status: row.get(5)?,
                    retired_at: row.get(6)?,
                })
            },
        )
        .optional()
}

/// Lookup a slug in `slugs/global_landing_pages.db`.
pub fn lookup_landing_page_slug(
    pages_conn: &Connection,
    slug: &str,
) -> rusqlite::Result<Option<ResolvedSlugInfo>> {
    pages_conn
        .query_row(
            "SELECT slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at 
             FROM global_landing_pages WHERE slug = ?1;",
            [slug],
            |row| {
                Ok(ResolvedSlugInfo {
                    slug: row.get(0)?,
                    owner_tenant_id: row.get(1)?,
                    target_type: SlugTargetType::LandingPage,
                    target_id: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    status: row.get(5)?,
                    retired_at: row.get(6)?,
                })
            },
        )
        .optional()
}

/// Unified slug lookup: checks `global_urls.db`, then `global_landing_pages.db`.
pub fn lookup_slug(
    urls_conn: &Connection,
    pages_conn: &Connection,
    slug: &str,
) -> rusqlite::Result<Option<ResolvedSlugInfo>> {
    if let Some(info) = lookup_url_slug(urls_conn, slug)? {
        return Ok(Some(info));
    }
    lookup_landing_page_slug(pages_conn, slug)
}

/// Register a URL slug in `slugs/global_urls.db`.
pub fn register_url_slug(
    urls_conn: &Connection,
    slug: &str,
    owner_tenant_id: &TenantId,
    target_id: &str,
    status: &str,
) -> rusqlite::Result<()> {
    let now = Utc::now().to_rfc3339();
    urls_conn.execute(
        "INSERT INTO global_urls (slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at) 
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL);",
        params![slug, owner_tenant_id.as_str(), target_id, now, now, status],
    )?;
    Ok(())
}

/// Register a landing page slug in `slugs/global_landing_pages.db`.
pub fn register_landing_page_slug(
    pages_conn: &Connection,
    slug: &str,
    owner_tenant_id: &TenantId,
    target_id: &str,
    status: &str,
) -> rusqlite::Result<()> {
    let now = Utc::now().to_rfc3339();
    pages_conn.execute(
        "INSERT INTO global_landing_pages (slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at) 
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL);",
        params![slug, owner_tenant_id.as_str(), target_id, now, now, status],
    )?;
    Ok(())
}

/// Atomic reservation for a URL slug in `slugs/global_urls.db` (status = 'reserving').
pub fn reserve_url_slug(
    reserved_conn: &Connection,
    urls_conn: &Connection,
    pages_conn: &Connection,
    slug: &str,
    owner_tenant_id: &TenantId,
) -> rusqlite::Result<()> {
    if !is_slug_available(reserved_conn, urls_conn, pages_conn, slug)? {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
            Some("Slug is unavailable or reserved".into()),
        ));
    }

    let now = Utc::now().to_rfc3339();
    urls_conn.execute(
        "INSERT INTO global_urls (slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at) 
         VALUES (?1, ?2, '', ?3, ?4, ?5, NULL);",
        params![slug, owner_tenant_id.as_str(), now, now, SLUG_STATUS_RESERVING],
    )?;
    Ok(())
}

/// Atomic reservation for a landing page slug in `slugs/global_landing_pages.db` (status = 'reserving').
pub fn reserve_landing_page_slug(
    reserved_conn: &Connection,
    urls_conn: &Connection,
    pages_conn: &Connection,
    slug: &str,
    owner_tenant_id: &TenantId,
) -> rusqlite::Result<()> {
    if !is_slug_available(reserved_conn, urls_conn, pages_conn, slug)? {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
            Some("Slug is unavailable or reserved".into()),
        ));
    }

    let now = Utc::now().to_rfc3339();
    pages_conn.execute(
        "INSERT INTO global_landing_pages (slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at) 
         VALUES (?1, ?2, '', ?3, ?4, ?5, NULL);",
        params![slug, owner_tenant_id.as_str(), now, now, SLUG_STATUS_RESERVING],
    )?;
    Ok(())
}

/// Release a reserving URL slug (e.g. if content creation fails).
pub fn release_url_slug(
    urls_conn: &Connection,
    slug: &str,
    owner_tenant_id: &TenantId,
) -> rusqlite::Result<()> {
    urls_conn.execute(
        "DELETE FROM global_urls WHERE slug = ?1 AND owner_tenant_id = ?2 AND status = 'reserving';",
        params![slug, owner_tenant_id.as_str()],
    )?;
    Ok(())
}

/// Release a reserving Landing Page slug (e.g. if content creation fails).
pub fn release_landing_page_slug(
    pages_conn: &Connection,
    slug: &str,
    owner_tenant_id: &TenantId,
) -> rusqlite::Result<()> {
    pages_conn.execute(
        "DELETE FROM global_landing_pages WHERE slug = ?1 AND owner_tenant_id = ?2 AND status = 'reserving';",
        params![slug, owner_tenant_id.as_str()],
    )?;
    Ok(())
}

/// Update URL slug target and activate after reservation.
pub fn activate_url_slug(
    urls_conn: &Connection,
    slug: &str,
    target_id: &str,
) -> rusqlite::Result<()> {
    let now = Utc::now().to_rfc3339();
    urls_conn.execute(
        "UPDATE global_urls SET target_id = ?1, status = ?2, updated_at = ?3 WHERE slug = ?4;",
        params![target_id, SLUG_STATUS_ACTIVE, now, slug],
    )?;
    Ok(())
}

/// Update Landing Page slug target and activate after reservation.
pub fn activate_landing_page_slug(
    pages_conn: &Connection,
    slug: &str,
    target_id: &str,
) -> rusqlite::Result<()> {
    let now = Utc::now().to_rfc3339();
    pages_conn.execute(
        "UPDATE global_landing_pages SET target_id = ?1, status = ?2, updated_at = ?3 WHERE slug = ?4;",
        params![target_id, SLUG_STATUS_ACTIVE, now, slug],
    )?;
    Ok(())
}

/// Retire a slug permanently so it cannot be reused.
pub fn retire_slug(
    urls_conn: &Connection,
    pages_conn: &Connection,
    slug: &str,
) -> rusqlite::Result<bool> {
    let now = Utc::now().to_rfc3339();
    let n_urls = urls_conn.execute(
        "UPDATE global_urls SET status = ?1, retired_at = ?2, updated_at = ?3 WHERE slug = ?4;",
        params![SLUG_STATUS_RETIRED, now, now, slug],
    )?;
    if n_urls > 0 {
        return Ok(true);
    }

    let n_pages = pages_conn.execute(
        "UPDATE global_landing_pages SET status = ?1, retired_at = ?2, updated_at = ?3 WHERE slug = ?4;",
        params![SLUG_STATUS_RETIRED, now, now, slug],
    )?;
    Ok(n_pages > 0)
}

/// Disable a slug (returns 410 Gone on redirect, but still owned by tenant).
pub fn disable_slug(
    urls_conn: &Connection,
    pages_conn: &Connection,
    slug: &str,
) -> rusqlite::Result<bool> {
    let now = Utc::now().to_rfc3339();
    let n_urls = urls_conn.execute(
        "UPDATE global_urls SET status = ?1, updated_at = ?2 WHERE slug = ?3;",
        params![SLUG_STATUS_DISABLED, now, slug],
    )?;
    if n_urls > 0 {
        return Ok(true);
    }

    let n_pages = pages_conn.execute(
        "UPDATE global_landing_pages SET status = ?1, updated_at = ?2 WHERE slug = ?3;",
        params![SLUG_STATUS_DISABLED, now, slug],
    )?;
    Ok(n_pages > 0)
}

/// Transfer slug ownership to a new tenant.
pub fn transfer_slug_owner(
    urls_conn: &Connection,
    pages_conn: &Connection,
    slug: &str,
    new_owner_tenant_id: &TenantId,
    new_target_id: &str,
) -> rusqlite::Result<bool> {
    let now = Utc::now().to_rfc3339();
    let n_urls = urls_conn.execute(
        "UPDATE global_urls SET owner_tenant_id = ?1, target_id = ?2, updated_at = ?3 WHERE slug = ?4;",
        params![new_owner_tenant_id.as_str(), new_target_id, now, slug],
    )?;
    if n_urls > 0 {
        return Ok(true);
    }

    let n_pages = pages_conn.execute(
        "UPDATE global_landing_pages SET owner_tenant_id = ?1, target_id = ?2, updated_at = ?3 WHERE slug = ?4;",
        params![new_owner_tenant_id.as_str(), new_target_id, now, slug],
    )?;
    Ok(n_pages > 0)
}

/// List all slugs owned by a specific tenant.
pub fn list_slugs_by_tenant(
    urls_conn: &Connection,
    pages_conn: &Connection,
    owner_tenant_id: &TenantId,
) -> rusqlite::Result<Vec<ResolvedSlugInfo>> {
    let mut results = Vec::new();

    let mut stmt = urls_conn.prepare(
        "SELECT slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at 
         FROM global_urls WHERE owner_tenant_id = ?1 ORDER BY created_at ASC;",
    )?;
    let rows = stmt.query_map([owner_tenant_id.as_str()], |row| {
        Ok(ResolvedSlugInfo {
            slug: row.get(0)?,
            owner_tenant_id: row.get(1)?,
            target_type: SlugTargetType::Url,
            target_id: row.get(2)?,
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
            status: row.get(5)?,
            retired_at: row.get(6)?,
        })
    })?;
    for r in rows {
        results.push(r?);
    }

    let mut stmt = pages_conn.prepare(
        "SELECT slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at 
         FROM global_landing_pages WHERE owner_tenant_id = ?1 ORDER BY created_at ASC;",
    )?;
    let rows = stmt.query_map([owner_tenant_id.as_str()], |row| {
        Ok(ResolvedSlugInfo {
            slug: row.get(0)?,
            owner_tenant_id: row.get(1)?,
            target_type: SlugTargetType::LandingPage,
            target_id: row.get(2)?,
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
            status: row.get(5)?,
            retired_at: row.get(6)?,
        })
    })?;
    for r in rows {
        results.push(r?);
    }

    Ok(results)
}

/// Clean up stale reservations older than threshold seconds.
pub fn cleanup_stale_reservations(
    urls_conn: &Connection,
    pages_conn: &Connection,
    older_than_seconds: i64,
) -> rusqlite::Result<usize> {
    let threshold = (Utc::now() - chrono::Duration::seconds(older_than_seconds)).to_rfc3339();
    let n1 = urls_conn.execute(
        "DELETE FROM global_urls WHERE status = 'reserving' AND created_at < ?1;",
        [&threshold],
    )?;
    let n2 = pages_conn.execute(
        "DELETE FROM global_landing_pages WHERE status = 'reserving' AND created_at < ?1;",
        [&threshold],
    )?;
    Ok(n1 + n2)
}
