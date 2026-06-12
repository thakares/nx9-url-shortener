use crate::models::LinkPreview;
use rusqlite::{params, Connection};
use uuid::Uuid;

/// Insert or update a link preview for a URL.
pub fn upsert_preview(
    conn: &Connection,
    url_id: &str,
    title: Option<&str>,
    description: Option<&str>,
    logo_url: Option<&str>,
    button_text: Option<&str>,
) -> rusqlite::Result<LinkPreview> {
    let id = Uuid::new_v4().to_string();
    let btn = button_text.unwrap_or("Continue");

    conn.execute(
        "INSERT INTO link_preview (id, url_id, title, description, logo_url, button_text)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(url_id) DO UPDATE SET
            title = excluded.title,
            description = excluded.description,
            logo_url = excluded.logo_url,
            button_text = excluded.button_text;",
        params![id, url_id, title, description, logo_url, btn],
    )?;

    // Return the current state (may have been an update with a different id)
    get_preview(conn, url_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

/// Get the link preview for a URL.
pub fn get_preview(conn: &Connection, url_id: &str) -> rusqlite::Result<Option<LinkPreview>> {
    let mut stmt = conn.prepare(
        "SELECT id, url_id, title, description, logo_url, button_text FROM link_preview WHERE url_id = ?1;"
    )?;
    let mut rows = stmt.query(params![url_id])?;

    if let Some(row) = rows.next()? {
        Ok(Some(LinkPreview {
            id: row.get(0)?,
            url_id: row.get(1)?,
            title: row.get(2)?,
            description: row.get(3)?,
            logo_url: row.get(4)?,
            button_text: row
                .get::<_, Option<String>>(5)?
                .unwrap_or_else(|| "Continue".to_string()),
        }))
    } else {
        Ok(None)
    }
}

/// Delete the link preview for a URL.
pub fn delete_preview(conn: &Connection, url_id: &str) -> rusqlite::Result<bool> {
    let count = conn.execute(
        "DELETE FROM link_preview WHERE url_id = ?1;",
        params![url_id],
    )?;
    Ok(count > 0)
}
