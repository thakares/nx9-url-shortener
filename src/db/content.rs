use crate::models::Url;
use chrono::Utc;
use rusqlite::{params, Connection};
use uuid::Uuid;

// Helper: Associate tags with a URL
fn associate_tags(conn: &Connection, url_id: &str, tags: &[String]) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM url_tags WHERE url_id = ?1;", params![url_id])?;
    for tag_name in tags {
        let tag_name = tag_name.trim().to_lowercase();
        if tag_name.is_empty() {
            continue;
        }

        // Insert tag if it doesn't exist
        conn.execute(
            "INSERT OR IGNORE INTO tags (id, name) VALUES (?1, ?2);",
            params![Uuid::new_v4().to_string(), tag_name],
        )?;

        // Get tag id
        let tag_id: String = conn.query_row(
            "SELECT id FROM tags WHERE name = ?1;",
            params![tag_name],
            |row| row.get(0),
        )?;

        // Insert association
        conn.execute(
            "INSERT OR IGNORE INTO url_tags (url_id, tag_id) VALUES (?1, ?2);",
            params![url_id, tag_id],
        )?;
    }
    Ok(())
}

// Helper: Get tags for a URL
pub fn get_tags_for_url(conn: &Connection, url_id: &str) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT t.name FROM tags t JOIN url_tags ut ON t.id = ut.tag_id WHERE ut.url_id = ?1 ORDER BY t.name;"
    )?;
    let rows = stmt.query_map(params![url_id], |row| row.get::<_, String>(0))?;
    let mut tags = Vec::new();
    for tag in rows {
        tags.push(tag?);
    }
    Ok(tags)
}

/// The full column list used in all URL SELECT queries.
const URL_COLUMNS: &str = "id, code, destination, title, description, status, created_at, updated_at, expires_at, expired, password_hash, last_status, last_latency_ms, max_access_count, access_count";

/// Build a Url struct from a row containing URL_COLUMNS in order.
fn url_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Url> {
    Ok(Url {
        id: row.get(0)?,
        code: row.get(1)?,
        destination: row.get(2)?,
        title: row.get(3)?,
        description: row.get(4)?,
        status: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        expires_at: row.get(8)?,
        expired: row.get::<_, i32>(9).unwrap_or(0) != 0,
        password_hash: row.get(10)?,
        last_status: row.get(11)?,
        last_latency_ms: row.get(12)?,
        max_access_count: row.get(13)?,
        access_count: row.get::<_, i64>(14).unwrap_or(0),
        tags: Vec::new(), // filled after query
    })
}

pub fn create_url(
    conn: &Connection,
    code: &str,
    destination: &str,
    title: Option<&str>,
    description: Option<&str>,
    tags: &[String],
) -> rusqlite::Result<Url> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let status = "healthy".to_string();

    conn.execute(
        "INSERT INTO urls (id, code, destination, title, description, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8);",
        params![id, code, destination, title, description, status, now, now],
    )?;

    associate_tags(conn, &id, tags)?;

    Ok(Url {
        id,
        code: code.to_string(),
        destination: destination.to_string(),
        title: title.map(|s| s.to_string()),
        description: description.map(|s| s.to_string()),
        status,
        created_at: now.clone(),
        updated_at: now,
        tags: tags.to_vec(),
        expires_at: None,
        expired: false,
        password_hash: None,
        last_status: None,
        last_latency_ms: None,
        max_access_count: None,
        access_count: 0,
    })
}

/// Create a URL with all extended options.
#[allow(clippy::too_many_arguments)]
pub fn create_url_extended(
    conn: &Connection,
    code: &str,
    destination: &str,
    title: Option<&str>,
    description: Option<&str>,
    tags: &[String],
    expires_at: Option<&str>,
    password_hash: Option<&str>,
    max_access_count: Option<i64>,
) -> rusqlite::Result<Url> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let status = "healthy".to_string();

    conn.execute(
        "INSERT INTO urls (id, code, destination, title, description, status, created_at, updated_at, expires_at, password_hash, max_access_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11);",
        params![id, code, destination, title, description, status, now, now, expires_at, password_hash, max_access_count],
    )?;

    associate_tags(conn, &id, tags)?;

    Ok(Url {
        id,
        code: code.to_string(),
        destination: destination.to_string(),
        title: title.map(|s| s.to_string()),
        description: description.map(|s| s.to_string()),
        status,
        created_at: now.clone(),
        updated_at: now,
        tags: tags.to_vec(),
        expires_at: expires_at.map(|s| s.to_string()),
        expired: false,
        password_hash: password_hash.map(|s| s.to_string()),
        last_status: None,
        last_latency_ms: None,
        max_access_count,
        access_count: 0,
    })
}

pub fn get_url_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<Url>> {
    let sql = format!("SELECT {} FROM urls WHERE id = ?1;", URL_COLUMNS);
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![id])?;

    if let Some(row) = rows.next()? {
        let mut url = url_from_row(row)?;
        url.tags = get_tags_for_url(conn, &url.id)?;
        Ok(Some(url))
    } else {
        Ok(None)
    }
}

pub fn get_url_by_code(conn: &Connection, code: &str) -> rusqlite::Result<Option<Url>> {
    let sql = format!("SELECT {} FROM urls WHERE code = ?1;", URL_COLUMNS);
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![code])?;

    if let Some(row) = rows.next()? {
        let mut url = url_from_row(row)?;
        url.tags = get_tags_for_url(conn, &url.id)?;
        Ok(Some(url))
    } else {
        Ok(None)
    }
}

pub fn update_url(
    conn: &Connection,
    id: &str,
    destination: &str,
    title: Option<&str>,
    description: Option<&str>,
    status: &str,
    tags: &[String],
) -> rusqlite::Result<Option<Url>> {
    let now = Utc::now().to_rfc3339();

    let count = conn.execute(
        "UPDATE urls SET destination = ?1, title = ?2, description = ?3, status = ?4, updated_at = ?5 WHERE id = ?6;",
        params![destination, title, description, status, now, id],
    )?;

    if count == 0 {
        return Ok(None);
    }

    associate_tags(conn, id, tags)?;

    get_url_by_id(conn, id)
}

pub fn delete_url(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let count = conn.execute("DELETE FROM urls WHERE id = ?1;", params![id])?;
    Ok(count > 0)
}

pub fn list_urls(
    conn: &Connection,
    limit: i64,
    offset: i64,
    tag_filter: Option<&str>,
) -> rusqlite::Result<Vec<Url>> {
    let mut urls = Vec::new();

    if let Some(tag) = tag_filter {
        let tag_name = tag.trim().to_lowercase();
        let sql = format!(
            "SELECT u.{} FROM urls u
             JOIN url_tags ut ON u.id = ut.url_id
             JOIN tags t ON ut.tag_id = t.id
             WHERE t.name = ?1
             ORDER BY u.created_at DESC LIMIT ?2 OFFSET ?3;",
            URL_COLUMNS
                .replace("id,", "u.id,")
                .replace(", code", ", u.code")
                .replace(", destination", ", u.destination")
                .replace(", title", ", u.title")
                .replace(", description", ", u.description")
                .replace(", status", ", u.status")
                .replace(", created_at", ", u.created_at")
                .replace(", updated_at", ", u.updated_at")
                .replace(", expires_at", ", u.expires_at")
                .replace(", expired", ", u.expired")
                .replace(", password_hash", ", u.password_hash")
                .replace(", last_status", ", u.last_status")
                .replace(", last_latency_ms", ", u.last_latency_ms")
                .replace(", max_access_count", ", u.max_access_count")
                .replace(", access_count", ", u.access_count")
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![tag_name, limit, offset], url_from_row)?;

        for r in rows {
            let mut url = r?;
            url.tags = get_tags_for_url(conn, &url.id)?;
            urls.push(url);
        }
    } else {
        let sql = format!(
            "SELECT {} FROM urls ORDER BY created_at DESC LIMIT ?1 OFFSET ?2;",
            URL_COLUMNS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![limit, offset], url_from_row)?;

        for r in rows {
            let mut url = r?;
            url.tags = get_tags_for_url(conn, &url.id)?;
            urls.push(url);
        }
    }

    Ok(urls)
}

pub fn list_urls_for_health_check(conn: &Connection) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare("SELECT id, destination FROM urls WHERE expired = 0;")?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    let mut res = Vec::new();
    for r in rows {
        res.push(r?);
    }
    Ok(res)
}

pub fn update_url_health(conn: &Connection, id: &str, status: &str) -> rusqlite::Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE urls SET status = ?1, updated_at = ?2 WHERE id = ?3;",
        params![status, now, id],
    )?;
    Ok(())
}

/// Update URL health with extended status and latency information.
pub fn update_url_health_extended(
    conn: &Connection,
    id: &str,
    status: &str,
    last_status: &str,
    latency_ms: Option<i64>,
) -> rusqlite::Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE urls SET status = ?1, last_status = ?2, last_latency_ms = ?3, updated_at = ?4 WHERE id = ?5;",
        params![status, last_status, latency_ms, now, id],
    )?;
    Ok(())
}

pub fn get_url_counts(conn: &Connection) -> rusqlite::Result<(i64, i64, i64)> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM urls;", [], |row| row.get(0))?;
    let active: i64 = conn.query_row(
        "SELECT COUNT(*) FROM urls WHERE status IN ('healthy', 'suspect') AND expired = 0;",
        [],
        |row| row.get(0),
    )?;
    let dead: i64 = conn.query_row(
        "SELECT COUNT(*) FROM urls WHERE status = 'dead' OR expired = 1;",
        [],
        |row| row.get(0),
    )?;
    Ok((total, active, dead))
}

/// Mark all URLs with expires_at < now as expired.
pub fn expire_urls(conn: &Connection) -> rusqlite::Result<usize> {
    let now = Utc::now().to_rfc3339();
    let count = conn.execute(
        "UPDATE urls SET expired = 1, updated_at = ?1 WHERE expires_at IS NOT NULL AND expires_at < ?1 AND expired = 0;",
        params![now],
    )?;
    Ok(count)
}

/// Set a password hash on a URL.
pub fn set_url_password(
    conn: &Connection,
    id: &str,
    password_hash: &str,
) -> rusqlite::Result<bool> {
    let now = Utc::now().to_rfc3339();
    let count = conn.execute(
        "UPDATE urls SET password_hash = ?1, updated_at = ?2 WHERE id = ?3;",
        params![password_hash, now, id],
    )?;
    Ok(count > 0)
}

/// Remove the password from a URL.
pub fn remove_url_password(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let now = Utc::now().to_rfc3339();
    let count = conn.execute(
        "UPDATE urls SET password_hash = NULL, updated_at = ?1 WHERE id = ?2;",
        params![now, id],
    )?;
    Ok(count > 0)
}

/// Atomically increment the access count and return the new value.
pub fn increment_access_count(conn: &Connection, id: &str) -> rusqlite::Result<i64> {
    conn.execute(
        "UPDATE urls SET access_count = access_count + 1 WHERE id = ?1;",
        params![id],
    )?;
    conn.query_row(
        "SELECT access_count FROM urls WHERE id = ?1;",
        params![id],
        |row| row.get(0),
    )
}

/// Set expiry on a URL.
pub fn set_url_expiry(conn: &Connection, id: &str, expires_at: &str) -> rusqlite::Result<bool> {
    let now = Utc::now().to_rfc3339();
    let count = conn.execute(
        "UPDATE urls SET expires_at = ?1, updated_at = ?2 WHERE id = ?3;",
        params![expires_at, now, id],
    )?;
    Ok(count > 0)
}

/// Get health status summary across all URLs.
pub fn get_health_summary(conn: &Connection) -> rusqlite::Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(last_status, status) AS health, COUNT(*) FROM urls GROUP BY health ORDER BY COUNT(*) DESC;"
    )?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    let mut res = Vec::new();
    for r in rows {
        res.push(r?);
    }
    Ok(res)
}

// --- Landing Page Operations (unchanged) ---

use crate::models::LandingPage;

pub fn create_landing_page(
    conn: &Connection,
    code: &str,
    slug: &str,
    title: &str,
    html_content: &str,
    state: &str,
) -> rusqlite::Result<LandingPage> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO landing_pages (id, code, slug, title, html_content, state, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8);",
        params![id, code, slug, title, html_content, state, now, now],
    )?;

    Ok(LandingPage {
        id,
        code: code.to_string(),
        slug: slug.to_string(),
        title: title.to_string(),
        html_content: html_content.to_string(),
        state: state.to_string(),
        created_at: now.clone(),
        updated_at: now,
    })
}

pub fn get_landing_page_by_id(
    conn: &Connection,
    id: &str,
) -> rusqlite::Result<Option<LandingPage>> {
    let mut stmt = conn.prepare(
        "SELECT id, code, slug, title, html_content, state, created_at, updated_at FROM landing_pages WHERE id = ?1;"
    )?;
    let mut rows = stmt.query(params![id])?;

    if let Some(row) = rows.next()? {
        Ok(Some(LandingPage {
            id: row.get(0)?,
            code: row.get(1)?,
            slug: row.get(2)?,
            title: row.get(3)?,
            html_content: row.get(4)?,
            state: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        }))
    } else {
        Ok(None)
    }
}

pub fn get_landing_page_by_code(
    conn: &Connection,
    code: &str,
) -> rusqlite::Result<Option<LandingPage>> {
    let mut stmt = conn.prepare(
        "SELECT id, code, slug, title, html_content, state, created_at, updated_at FROM landing_pages WHERE code = ?1;"
    )?;
    let mut rows = stmt.query(params![code])?;

    if let Some(row) = rows.next()? {
        Ok(Some(LandingPage {
            id: row.get(0)?,
            code: row.get(1)?,
            slug: row.get(2)?,
            title: row.get(3)?,
            html_content: row.get(4)?,
            state: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        }))
    } else {
        Ok(None)
    }
}

pub fn update_landing_page(
    conn: &Connection,
    id: &str,
    slug: &str,
    title: &str,
    html_content: &str,
    state: &str,
) -> rusqlite::Result<Option<LandingPage>> {
    let now = Utc::now().to_rfc3339();

    let count = conn.execute(
        "UPDATE landing_pages SET slug = ?1, title = ?2, html_content = ?3, state = ?4, updated_at = ?5 WHERE id = ?6;",
        params![slug, title, html_content, state, now, id],
    )?;

    if count == 0 {
        return Ok(None);
    }

    get_landing_page_by_id(conn, id)
}

pub fn delete_landing_page(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let count = conn.execute("DELETE FROM landing_pages WHERE id = ?1;", params![id])?;
    Ok(count > 0)
}

pub fn list_landing_pages(
    conn: &Connection,
    limit: i64,
    offset: i64,
) -> rusqlite::Result<Vec<LandingPage>> {
    let mut stmt = conn.prepare(
        "SELECT id, code, slug, title, html_content, state, created_at, updated_at 
         FROM landing_pages ORDER BY created_at DESC LIMIT ?1 OFFSET ?2;",
    )?;
    let rows = stmt.query_map(params![limit, offset], |row| {
        Ok(LandingPage {
            id: row.get(0)?,
            code: row.get(1)?,
            slug: row.get(2)?,
            title: row.get(3)?,
            html_content: row.get(4)?,
            state: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    })?;

    let mut pages = Vec::new();
    for page in rows {
        pages.push(page?);
    }
    Ok(pages)
}

pub fn get_landing_page_count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM landing_pages;", [], |row| row.get(0))
}
