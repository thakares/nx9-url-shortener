use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

/// Log a QR code access event to the analytics database.
pub fn log_qr_access(
    conn: &Connection,
    url_id: &str,
    ip: Option<&str>,
    user_agent: Option<&str>,
) -> rusqlite::Result<()> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO qr_access_log (id, url_id, timestamp, ip, user_agent)
         VALUES (?1, ?2, ?3, ?4, ?5);",
        params![id, url_id, now, ip, user_agent],
    )?;
    Ok(())
}

/// Get QR scan count for a URL.
pub fn get_qr_scan_count(conn: &Connection, url_id: &str) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM qr_access_log WHERE url_id = ?1;",
        params![url_id],
        |row| row.get(0),
    )
}

/// Get QR scan count for a URL by its code (joins with content.db — must be called on analytics db after lookup).
pub fn get_qr_stats_for_url(
    conn: &Connection,
    url_id: &str,
) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT timestamp, ip FROM qr_access_log WHERE url_id = ?1 ORDER BY timestamp DESC LIMIT 100;"
    )?;
    let rows = stmt.query_map(params![url_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?.unwrap_or_default(),
        ))
    })?;
    let mut results = Vec::new();
    for r in rows {
        results.push(r?);
    }
    Ok(results)
}

/// Create or update a QR code style registration in the content database.
pub fn upsert_qr_code(conn: &Connection, url_id: &str, style: &str) -> rusqlite::Result<()> {
    let now = Utc::now().to_rfc3339();
    // Check if entry already exists
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM qr_codes WHERE url_id = ?1;",
            params![url_id],
            |row| row.get(0),
        )
        .optional()?;

    if let Some(id) = existing_id {
        conn.execute(
            "UPDATE qr_codes SET style = ?1 WHERE id = ?2;",
            params![style, id],
        )?;
    } else {
        let id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO qr_codes (id, url_id, style, created_at) VALUES (?1, ?2, ?3, ?4);",
            params![id, url_id, style, now],
        )?;
    }
    Ok(())
}

/// Get the style configured for a QR code.
pub fn get_qr_code_style(conn: &Connection, url_id: &str) -> rusqlite::Result<String> {
    let style: Option<String> = conn
        .query_row(
            "SELECT style FROM qr_codes WHERE url_id = ?1;",
            params![url_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(style.unwrap_or_else(|| "default".to_string()))
}
