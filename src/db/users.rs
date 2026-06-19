use crate::models::{TenantUser, UserApiToken, UserQuotas, UserSession};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

// --- User Operations ---

pub fn is_reserved_username(username: &str) -> bool {
    let u = username.trim().to_lowercase();
    u == "admin" || u == "legacy_admin" || u == "administrator" || u == "system" || u == "root"
}

pub fn create_admin_user(
    conn: &Connection,
    username: &str,
    password_hash: &str,
) -> rusqlite::Result<TenantUser> {
    let created_at = Utc::now().to_rfc3339();
    let status = "active";
    let account_type = "admin";

    conn.execute(
        "INSERT INTO users (username, password_hash, status, created_at, account_type, metadata) 
         VALUES (?1, ?2, ?3, ?4, ?5, NULL);",
        params![username, password_hash, status, created_at, account_type],
    )?;

    let id = conn.last_insert_rowid();

    // Seed default quotas
    conn.execute("INSERT INTO quotas (user_id) VALUES (?1);", params![id])?;

    Ok(TenantUser {
        id,
        username: username.to_string(),
        password_hash: password_hash.to_string(),
        status: status.to_string(),
        created_at,
        last_login: None,
        account_type: account_type.to_string(),
        organization_id: None,
        metadata: None,
    })
}

pub fn create_user(
    conn: &Connection,
    username: &str,
    password_hash: &str,
    account_type: &str,
    metadata: Option<&str>,
) -> rusqlite::Result<TenantUser> {
    if is_reserved_username(username) {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
            Some("Username is reserved".to_string()),
        ));
    }

    let created_at = Utc::now().to_rfc3339();
    let status = "active";

    conn.execute(
        "INSERT INTO users (username, password_hash, status, created_at, account_type, metadata) 
         VALUES (?1, ?2, ?3, ?4, ?5, ?6);",
        params![
            username,
            password_hash,
            status,
            created_at,
            account_type,
            metadata
        ],
    )?;

    let id = conn.last_insert_rowid();

    // Seed default quotas
    conn.execute("INSERT INTO quotas (user_id) VALUES (?1);", params![id])?;

    Ok(TenantUser {
        id,
        username: username.to_string(),
        password_hash: password_hash.to_string(),
        status: status.to_string(),
        created_at,
        last_login: None,
        account_type: account_type.to_string(),
        organization_id: None,
        metadata: metadata.map(|s| s.to_string()),
    })
}

pub fn get_user_by_id(conn: &Connection, id: i64) -> rusqlite::Result<Option<TenantUser>> {
    conn.query_row(
        "SELECT id, username, password_hash, status, created_at, last_login, account_type, organization_id, metadata 
         FROM users WHERE id = ?1;",
        params![id],
        |row| {
            Ok(TenantUser {
                id: row.get(0)?,
                username: row.get(1)?,
                password_hash: row.get(2)?,
                status: row.get(3)?,
                created_at: row.get(4)?,
                last_login: row.get(5)?,
                account_type: row.get(6)?,
                organization_id: row.get(7)?,
                metadata: row.get(8)?,
            })
        },
    )
    .optional()
}

pub fn get_user_by_username(
    conn: &Connection,
    username: &str,
) -> rusqlite::Result<Option<TenantUser>> {
    conn.query_row(
        "SELECT id, username, password_hash, status, created_at, last_login, account_type, organization_id, metadata 
         FROM users WHERE username = ?1;",
        params![username],
        |row| {
            Ok(TenantUser {
                id: row.get(0)?,
                username: row.get(1)?,
                password_hash: row.get(2)?,
                status: row.get(3)?,
                created_at: row.get(4)?,
                last_login: row.get(5)?,
                account_type: row.get(6)?,
                organization_id: row.get(7)?,
                metadata: row.get(8)?,
            })
        },
    )
    .optional()
}

pub fn delete_user(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM users WHERE id = ?1;", params![id])?;
    Ok(())
}

pub fn update_user_status(conn: &Connection, id: i64, status: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE users SET status = ?1 WHERE id = ?2;",
        params![status, id],
    )?;
    Ok(())
}

pub fn update_user_account_type(
    conn: &Connection,
    id: i64,
    account_type: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE users SET account_type = ?1 WHERE id = ?2;",
        params![account_type, id],
    )?;
    Ok(())
}

pub fn reset_user_password(
    conn: &Connection,
    id: i64,
    new_password_hash: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE users SET password_hash = ?1 WHERE id = ?2;",
        params![new_password_hash, id],
    )?;
    Ok(())
}

pub fn update_user_last_login(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE users SET last_login = ?1 WHERE id = ?2;",
        params![now, id],
    )?;
    Ok(())
}

pub fn list_users(conn: &Connection) -> rusqlite::Result<Vec<TenantUser>> {
    let mut stmt = conn.prepare(
        "SELECT id, username, password_hash, status, created_at, last_login, account_type, organization_id, metadata 
         FROM users ORDER BY username ASC;",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(TenantUser {
            id: row.get(0)?,
            username: row.get(1)?,
            password_hash: row.get(2)?,
            status: row.get(3)?,
            created_at: row.get(4)?,
            last_login: row.get(5)?,
            account_type: row.get(6)?,
            organization_id: row.get(7)?,
            metadata: row.get(8)?,
        })
    })?;

    let mut users = Vec::new();
    for u in rows {
        users.push(u?);
    }
    Ok(users)
}

pub fn log_username_change(
    conn: &Connection,
    user_id: i64,
    old_username: &str,
    new_username: &str,
) -> rusqlite::Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO username_history (user_id, old_username, new_username, changed_at) VALUES (?1, ?2, ?3, ?4);",
        params![user_id, old_username, new_username, now],
    )?;
    conn.execute(
        "UPDATE users SET username = ?1 WHERE id = ?2;",
        params![new_username, user_id],
    )?;
    Ok(())
}

// --- Session Operations ---

pub fn create_user_session(
    conn: &Connection,
    session_id: &str,
    user_id: i64,
    expires_at_rfc3339: &str,
) -> rusqlite::Result<UserSession> {
    let created_at = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO sessions (id, user_id, expires_at, created_at) VALUES (?1, ?2, ?3, ?4);",
        params![session_id, user_id, expires_at_rfc3339, created_at],
    )?;
    Ok(UserSession {
        id: session_id.to_string(),
        user_id,
        expires_at: expires_at_rfc3339.to_string(),
        created_at,
    })
}

pub fn get_user_session(
    conn: &Connection,
    session_id: &str,
) -> rusqlite::Result<Option<UserSession>> {
    conn.query_row(
        "SELECT id, user_id, expires_at, created_at FROM sessions WHERE id = ?1;",
        params![session_id],
        |row| {
            Ok(UserSession {
                id: row.get(0)?,
                user_id: row.get(1)?,
                expires_at: row.get(2)?,
                created_at: row.get(3)?,
            })
        },
    )
    .optional()
}

pub fn delete_user_session(conn: &Connection, session_id: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM sessions WHERE id = ?1;", params![session_id])?;
    Ok(())
}

pub fn cleanup_expired_user_sessions(conn: &Connection) -> rusqlite::Result<usize> {
    let now = Utc::now().to_rfc3339();
    let count = conn.execute("DELETE FROM sessions WHERE expires_at < ?1;", params![now])?;
    Ok(count)
}

// --- Quota Operations ---

pub fn get_user_quotas(conn: &Connection, user_id: i64) -> rusqlite::Result<Option<UserQuotas>> {
    conn.query_row(
        "SELECT user_id, max_urls, max_landings, max_api_tokens, max_storage_mb, 
                current_urls, current_landings, current_api_tokens, current_storage_mb 
         FROM quotas WHERE user_id = ?1;",
        params![user_id],
        |row| {
            Ok(UserQuotas {
                user_id: row.get(0)?,
                max_urls: row.get(1)?,
                max_landings: row.get(2)?,
                max_api_tokens: row.get(3)?,
                max_storage_mb: row.get(4)?,
                current_urls: row.get(5)?,
                current_landings: row.get(6)?,
                current_api_tokens: row.get(7)?,
                current_storage_mb: row.get(8)?,
            })
        },
    )
    .optional()
}

pub fn update_user_quotas(
    conn: &Connection,
    user_id: i64,
    max_urls: i64,
    max_landings: i64,
    max_api_tokens: i64,
    max_storage_mb: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE quotas SET max_urls = ?1, max_landings = ?2, max_api_tokens = ?3, max_storage_mb = ?4 
         WHERE user_id = ?5;",
        params![max_urls, max_landings, max_api_tokens, max_storage_mb, user_id],
    )?;
    Ok(())
}

pub fn increment_quota_counter(
    conn: &Connection,
    user_id: i64,
    field: &str,
) -> rusqlite::Result<()> {
    let sql = match field {
        "urls" => "UPDATE quotas SET current_urls = current_urls + 1 WHERE user_id = ?1;",
        "landings" => {
            "UPDATE quotas SET current_landings = current_landings + 1 WHERE user_id = ?1;"
        }
        "api_tokens" => {
            "UPDATE quotas SET current_api_tokens = current_api_tokens + 1 WHERE user_id = ?1;"
        }
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    conn.execute(sql, params![user_id])?;
    Ok(())
}

pub fn decrement_quota_counter(
    conn: &Connection,
    user_id: i64,
    field: &str,
) -> rusqlite::Result<()> {
    let sql = match field {
        "urls" => "UPDATE quotas SET current_urls = MAX(0, current_urls - 1) WHERE user_id = ?1;",
        "landings" => "UPDATE quotas SET current_landings = MAX(0, current_landings - 1) WHERE user_id = ?1;",
        "api_tokens" => "UPDATE quotas SET current_api_tokens = MAX(0, current_api_tokens - 1) WHERE user_id = ?1;",
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    conn.execute(sql, params![user_id])?;
    Ok(())
}

pub fn update_quota_storage(
    conn: &Connection,
    user_id: i64,
    storage_mb: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE quotas SET current_storage_mb = ?1 WHERE user_id = ?2;",
        params![storage_mb, user_id],
    )?;
    Ok(())
}

// --- API Token Operations ---

pub fn create_user_api_token(
    conn: &Connection,
    user_id: i64,
    token_hash: &str,
) -> rusqlite::Result<UserApiToken> {
    let created_at = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO api_tokens (user_id, token_hash, created_at) VALUES (?1, ?2, ?3);",
        params![user_id, token_hash, created_at],
    )?;
    let id = conn.last_insert_rowid();

    // Increment api token counter
    let _ = increment_quota_counter(conn, user_id, "api_tokens");

    Ok(UserApiToken {
        id,
        user_id,
        token_hash: token_hash.to_string(),
        created_at,
    })
}

pub fn list_user_api_tokens(
    conn: &Connection,
    user_id: i64,
) -> rusqlite::Result<Vec<UserApiToken>> {
    let mut stmt = conn.prepare(
        "SELECT id, user_id, token_hash, created_at FROM api_tokens WHERE user_id = ?1 ORDER BY id DESC;",
    )?;
    let rows = stmt.query_map(params![user_id], |row| {
        Ok(UserApiToken {
            id: row.get(0)?,
            user_id: row.get(1)?,
            token_hash: row.get(2)?,
            created_at: row.get(3)?,
        })
    })?;

    let mut tokens = Vec::new();
    for t in rows {
        tokens.push(t?);
    }
    Ok(tokens)
}

pub fn delete_user_api_token(conn: &Connection, id: i64, user_id: i64) -> rusqlite::Result<()> {
    let deleted = conn.execute(
        "DELETE FROM api_tokens WHERE id = ?1 AND user_id = ?2;",
        params![id, user_id],
    )?;
    if deleted > 0 {
        let _ = decrement_quota_counter(conn, user_id, "api_tokens");
    }
    Ok(())
}

// --- Global Slug & Quota Reconciliation Helpers ---

pub fn is_slug_available(system_conn: &Connection, slug: &str) -> rusqlite::Result<bool> {
    // 1. Check reserved list
    let reserved: bool = system_conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM reserved_slugs WHERE slug = ?1);",
            [slug],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if reserved {
        return Ok(false);
    }

    // 2. Check global slugs
    let exists: bool = system_conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM global_slugs WHERE slug = ?1);",
            [slug],
            |row| row.get(0),
        )
        .unwrap_or(false);

    Ok(!exists)
}

pub fn register_global_slug(
    system_conn: &Connection,
    slug: &str,
    owner_user_id: i64,
    target_type: &str,
    target_id: &str,
) -> rusqlite::Result<()> {
    let now = Utc::now().to_rfc3339();
    system_conn.execute(
        "INSERT INTO global_slugs (slug, owner_user_id, target_type, target_id, created_at, updated_at, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);",
        rusqlite::params![slug, owner_user_id, target_type, target_id, now, now, "active"],
    )?;

    // Insert history
    system_conn.execute(
        "INSERT INTO slug_history (slug, old_owner_user_id, new_owner_user_id, action, timestamp)
         VALUES (?1, NULL, ?2, 'created', ?3);",
        rusqlite::params![slug, owner_user_id, now],
    )?;

    Ok(())
}

pub fn release_global_slug(
    system_conn: &Connection,
    slug: &str,
    owner_user_id: i64,
) -> rusqlite::Result<()> {
    let now = Utc::now().to_rfc3339();
    system_conn.execute("DELETE FROM global_slugs WHERE slug = ?1;", [slug])?;

    // Insert history
    system_conn.execute(
        "INSERT INTO slug_history (slug, old_owner_user_id, new_owner_user_id, action, timestamp)
         VALUES (?1, ?2, NULL, 'released', ?3);",
        rusqlite::params![slug, owner_user_id, now],
    )?;

    Ok(())
}

pub fn soft_delete_global_slug(
    system_conn: &Connection,
    slug: &str,
    owner_user_id: i64,
) -> rusqlite::Result<()> {
    let now = Utc::now().to_rfc3339();
    system_conn.execute(
        "UPDATE global_slugs SET status = 'soft_deleted', deleted_at = ?1 WHERE slug = ?2;",
        rusqlite::params![now, slug],
    )?;

    // Insert history
    system_conn.execute(
        "INSERT INTO slug_history (slug, old_owner_user_id, new_owner_user_id, action, timestamp)
         VALUES (?1, ?2, NULL, 'deleted', ?3);",
        rusqlite::params![slug, owner_user_id, now],
    )?;

    Ok(())
}

pub fn reconcile_user_quotas(
    users_conn: &Connection,
    user_id: i64,
    content_conn: &Connection,
) -> rusqlite::Result<()> {
    let urls_count: i64 = content_conn
        .query_row("SELECT COUNT(*) FROM urls;", [], |row| row.get(0))
        .unwrap_or(0);

    let landings_count: i64 = content_conn
        .query_row("SELECT COUNT(*) FROM landing_pages;", [], |row| row.get(0))
        .unwrap_or(0);

    let api_tokens_count: i64 = users_conn
        .query_row(
            "SELECT COUNT(*) FROM api_tokens WHERE user_id = ?1;",
            [user_id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    users_conn.execute(
        "UPDATE quotas SET current_urls = ?1, current_landings = ?2, current_api_tokens = ?3 WHERE user_id = ?4;",
        rusqlite::params![urls_count, landings_count, api_tokens_count, user_id],
    )?;

    Ok(())
}
