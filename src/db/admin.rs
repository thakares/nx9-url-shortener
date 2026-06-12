use crate::models::{ApiKey, AuditLog, Session, User};
use chrono::Utc;
use rusqlite::{params, Connection};
use uuid::Uuid;

pub fn create_user(
    conn: &Connection,
    username: &str,
    password_hash: &str,
) -> rusqlite::Result<User> {
    let id = Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO users (id, username, password_hash, created_at) VALUES (?1, ?2, ?3, ?4);",
        params![id, username, password_hash, created_at],
    )?;

    Ok(User {
        id,
        username: username.to_string(),
        password_hash: password_hash.to_string(),
        created_at,
    })
}

pub fn get_user_by_username(conn: &Connection, username: &str) -> rusqlite::Result<Option<User>> {
    let mut stmt = conn.prepare(
        "SELECT id, username, password_hash, created_at FROM users WHERE username = ?1;",
    )?;
    let mut rows = stmt.query(params![username])?;

    if let Some(row) = rows.next()? {
        Ok(Some(User {
            id: row.get(0)?,
            username: row.get(1)?,
            password_hash: row.get(2)?,
            created_at: row.get(3)?,
        }))
    } else {
        Ok(None)
    }
}

pub fn get_user_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<User>> {
    let mut stmt =
        conn.prepare("SELECT id, username, password_hash, created_at FROM users WHERE id = ?1;")?;
    let mut rows = stmt.query(params![id])?;

    if let Some(row) = rows.next()? {
        Ok(Some(User {
            id: row.get(0)?,
            username: row.get(1)?,
            password_hash: row.get(2)?,
            created_at: row.get(3)?,
        }))
    } else {
        Ok(None)
    }
}

pub fn get_user_count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM users;", [], |row| row.get(0))
}

pub fn create_session(
    conn: &Connection,
    session_id: &str,
    user_id: &str,
    expires_at_rfc3339: &str,
) -> rusqlite::Result<Session> {
    let created_at = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO sessions (id, user_id, expires_at, created_at) VALUES (?1, ?2, ?3, ?4);",
        params![session_id, user_id, expires_at_rfc3339, created_at],
    )?;

    Ok(Session {
        id: session_id.to_string(),
        user_id: user_id.to_string(),
        expires_at: expires_at_rfc3339.to_string(),
        created_at,
    })
}

pub fn get_session(conn: &Connection, session_id: &str) -> rusqlite::Result<Option<Session>> {
    let mut stmt =
        conn.prepare("SELECT id, user_id, expires_at, created_at FROM sessions WHERE id = ?1;")?;
    let mut rows = stmt.query(params![session_id])?;

    if let Some(row) = rows.next()? {
        Ok(Some(Session {
            id: row.get(0)?,
            user_id: row.get(1)?,
            expires_at: row.get(2)?,
            created_at: row.get(3)?,
        }))
    } else {
        Ok(None)
    }
}

pub fn delete_session(conn: &Connection, session_id: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM sessions WHERE id = ?1;", params![session_id])?;
    Ok(())
}

pub fn cleanup_expired_sessions(conn: &Connection) -> rusqlite::Result<usize> {
    let now = Utc::now().to_rfc3339();
    let count = conn.execute("DELETE FROM sessions WHERE expires_at < ?1;", params![now])?;
    Ok(count)
}

pub fn create_api_key(
    conn: &Connection,
    user_id: &str,
    name: &str,
    key_hash: &str,
) -> rusqlite::Result<ApiKey> {
    let id = Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO api_keys (id, user_id, key_hash, name, created_at) VALUES (?1, ?2, ?3, ?4, ?5);",
        params![id, user_id, key_hash, name, created_at],
    )?;

    Ok(ApiKey {
        id,
        user_id: user_id.to_string(),
        key_hash: key_hash.to_string(),
        name: name.to_string(),
        created_at,
        last_used_at: None,
    })
}

pub fn get_api_key_by_hash(conn: &Connection, key_hash: &str) -> rusqlite::Result<Option<ApiKey>> {
    let mut stmt = conn.prepare(
        "SELECT id, user_id, key_hash, name, created_at, last_used_at FROM api_keys WHERE key_hash = ?1;"
    )?;
    let mut rows = stmt.query(params![key_hash])?;

    if let Some(row) = rows.next()? {
        Ok(Some(ApiKey {
            id: row.get(0)?,
            user_id: row.get(1)?,
            key_hash: row.get(2)?,
            name: row.get(3)?,
            created_at: row.get(4)?,
            last_used_at: row.get(5)?,
        }))
    } else {
        Ok(None)
    }
}

pub fn list_api_keys(conn: &Connection, user_id: &str) -> rusqlite::Result<Vec<ApiKey>> {
    let mut stmt = conn.prepare(
        "SELECT id, user_id, key_hash, name, created_at, last_used_at FROM api_keys WHERE user_id = ?1 ORDER BY created_at DESC;"
    )?;
    let rows = stmt.query_map(params![user_id], |row| {
        Ok(ApiKey {
            id: row.get(0)?,
            user_id: row.get(1)?,
            key_hash: row.get(2)?,
            name: row.get(3)?,
            created_at: row.get(4)?,
            last_used_at: row.get(5)?,
        })
    })?;

    let mut keys = Vec::new();
    for key in rows {
        keys.push(key?);
    }
    Ok(keys)
}

pub fn delete_api_key(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM api_keys WHERE id = ?1;", params![id])?;
    Ok(())
}

pub fn update_api_key_last_used(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE api_keys SET last_used_at = ?1 WHERE id = ?2;",
        params![now, id],
    )?;
    Ok(())
}

pub fn write_audit_log(
    conn: &Connection,
    username: &str,
    action: &str,
    object_type: Option<&str>,
    object_id: Option<&str>,
    ip_address: Option<&str>,
    user_agent: Option<&str>,
) -> rusqlite::Result<AuditLog> {
    let id = Uuid::new_v4().to_string();
    let timestamp = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO audit_logs (id, timestamp, username, action, object_type, object_id, ip_address, user_agent) 
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8);",
        params![id, timestamp, username, action, object_type, object_id, ip_address, user_agent],
    )?;

    Ok(AuditLog {
        id,
        timestamp,
        username: username.to_string(),
        action: action.to_string(),
        object_type: object_type.map(|s| s.to_string()),
        object_id: object_id.map(|s| s.to_string()),
        ip_address: ip_address.map(|s| s.to_string()),
        user_agent: user_agent.map(|s| s.to_string()),
    })
}

pub fn list_audit_logs(
    conn: &Connection,
    limit: i64,
    offset: i64,
) -> rusqlite::Result<Vec<AuditLog>> {
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, username, action, object_type, object_id, ip_address, user_agent 
         FROM audit_logs ORDER BY timestamp DESC LIMIT ?1 OFFSET ?2;",
    )?;
    let rows = stmt.query_map(params![limit, offset], |row| {
        Ok(AuditLog {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            username: row.get(2)?,
            action: row.get(3)?,
            object_type: row.get(4)?,
            object_id: row.get(5)?,
            ip_address: row.get(6)?,
            user_agent: row.get(7)?,
        })
    })?;

    let mut logs = Vec::new();
    for log in rows {
        logs.push(log?);
    }
    Ok(logs)
}

pub fn set_config(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO config (key, value) VALUES (?1, ?2);",
        params![key, value],
    )?;
    Ok(())
}

pub fn get_config(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT value FROM config WHERE key = ?1;")?;
    let mut rows = stmt.query(params![key])?;

    if let Some(row) = rows.next()? {
        let val: String = row.get(0)?;
        Ok(Some(val))
    } else {
        Ok(None)
    }
}
