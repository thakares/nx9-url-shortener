use crate::identity::TenantId;
use crate::models::{TenantUser, UserApiToken, UserQuotas, UserSession};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

const USER_COLUMNS: &str = "id, username, password_hash, status, created_at, last_login, account_type, organization_id, metadata, tenant_id, uuid";

fn map_user(row: &rusqlite::Row<'_>) -> rusqlite::Result<TenantUser> {
    let tenant_raw: Option<String> = row.get(9)?;
    let tenant_id = match tenant_raw {
        Some(s) => Some(TenantId::parse(&s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(e))
        })?),
        None => None,
    };
    let uuid: Option<String> = row.get(10)?;
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
        tenant_id,
        uuid,
    })
}

fn allocate_unique_tenant_id(conn: &Connection) -> rusqlite::Result<TenantId> {
    for _ in 0..16 {
        let candidate = TenantId::generate();
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM users WHERE tenant_id = ?1);",
            [candidate.as_str()],
            |row| row.get(0),
        )?;
        if !exists {
            return Ok(candidate);
        }
    }
    Err(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
        Some("failed to allocate unique TenantId".into()),
    ))
}

pub fn allocate_unique_uuid(conn: &Connection) -> rusqlite::Result<String> {
    for _ in 0..16 {
        let candidate = uuid::Uuid::new_v4().to_string();
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM users WHERE uuid = ?1);",
            [&candidate],
            |row| row.get(0),
        )?;
        if !exists {
            return Ok(candidate);
        }
    }
    Err(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
        Some("failed to allocate unique UUID".into()),
    ))
}

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
    let user_uuid = allocate_unique_uuid(conn)?;

    conn.execute(
        "INSERT INTO users (username, password_hash, status, created_at, account_type, metadata, tenant_id, uuid) 
         VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, ?6);",
        params![
            username,
            password_hash,
            status,
            created_at,
            account_type,
            user_uuid,
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
        metadata: None,
        tenant_id: None,
        uuid: Some(user_uuid),
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
    let tenant_id = allocate_unique_tenant_id(conn)?;
    let user_uuid = allocate_unique_uuid(conn)?;

    conn.execute(
        "INSERT INTO users (username, password_hash, status, created_at, account_type, metadata, tenant_id, uuid) 
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8);",
        params![
            username,
            password_hash,
            status,
            created_at,
            account_type,
            metadata,
            tenant_id.as_str(),
            user_uuid,
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
        tenant_id: Some(tenant_id),
        uuid: Some(user_uuid),
    })
}

pub fn get_user_by_id(conn: &Connection, id: i64) -> rusqlite::Result<Option<TenantUser>> {
    conn.query_row(
        &format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?1;"),
        params![id],
        map_user,
    )
    .optional()
}

pub fn get_user_by_tenant_id(
    conn: &Connection,
    tenant_id: TenantId,
) -> rusqlite::Result<Option<TenantUser>> {
    conn.query_row(
        &format!("SELECT {USER_COLUMNS} FROM users WHERE tenant_id = ?1;"),
        params![tenant_id.as_str()],
        map_user,
    )
    .optional()
}

pub fn get_user_by_uuid(conn: &Connection, uuid: &str) -> rusqlite::Result<Option<TenantUser>> {
    conn.query_row(
        &format!("SELECT {USER_COLUMNS} FROM users WHERE uuid = ?1;"),
        params![uuid],
        map_user,
    )
    .optional()
}

pub fn get_user_by_username(
    conn: &Connection,
    username: &str,
) -> rusqlite::Result<Option<TenantUser>> {
    conn.query_row(
        &format!("SELECT {USER_COLUMNS} FROM users WHERE username = ?1;"),
        params![username],
        map_user,
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
    let mut stmt = conn.prepare(&format!(
        "SELECT {USER_COLUMNS} FROM users ORDER BY username ASC;"
    ))?;
    let rows = stmt.query_map([], map_user)?;

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

pub fn check_quota_limit(conn: &Connection, user_id: i64, field: &str) -> rusqlite::Result<bool> {
    if let Some(quotas) = get_user_quotas(conn, user_id)? {
        match field {
            "urls" => Ok(quotas.current_urls < quotas.max_urls),
            "landings" => Ok(quotas.current_landings < quotas.max_landings),
            "api_tokens" => Ok(quotas.current_api_tokens < quotas.max_api_tokens),
            _ => Ok(false),
        }
    } else {
        Ok(false)
    }
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

#[deprecated(
    note = "Legacy v0.7 global_slugs function; use crate::db::slugs::is_slug_available instead"
)]
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

#[deprecated(
    note = "Legacy v0.7 global_slugs function; use crate::db::slugs::reserve_*_slug instead"
)]
pub fn register_global_slug(
    system_conn: &Connection,
    slug: &str,
    owner_user_id: i64,
    target_type: &str,
    target_id: &str,
    status: &str,
) -> rusqlite::Result<()> {
    let now = Utc::now().to_rfc3339();
    system_conn.execute(
        "INSERT INTO global_slugs (slug, owner_user_id, target_type, target_id, created_at, updated_at, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);",
        rusqlite::params![slug, owner_user_id, target_type, target_id, now, now, status],
    )?;

    // Insert history
    system_conn.execute(
        "INSERT INTO slug_history (slug, old_owner_user_id, new_owner_user_id, action, timestamp)
         VALUES (?1, NULL, ?2, 'created', ?3);",
        rusqlite::params![slug, owner_user_id, now],
    )?;

    Ok(())
}

#[deprecated(
    note = "Legacy v0.7 global_slugs function; use crate::db::slugs::release_*_slug instead"
)]
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

#[deprecated(note = "Legacy v0.7 global_slugs function; use crate::db::slugs APIs instead")]
pub fn soft_delete_global_slug(
    system_conn: &Connection,
    slug: &str,
    owner_user_id: i64,
) -> rusqlite::Result<()> {
    let now = Utc::now().to_rfc3339();
    system_conn.execute(
        "UPDATE global_slugs SET status = 'disabled', deleted_at = ?1 WHERE slug = ?2;",
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

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SlugAuditReport {
    pub duplicates: Vec<String>,
    pub invalid_entries: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn audit_slug_namespace(
    config: &crate::config::Config,
) -> Result<SlugAuditReport, Box<dyn std::error::Error>> {
    use std::collections::HashMap;
    let mut duplicates = Vec::new();
    let mut invalid_entries = Vec::new();
    let warnings = Vec::new();

    let mut slug_owners: HashMap<String, Vec<String>> = HashMap::new();

    // 1. Scan legacy content.db if it exists
    let legacy_content_path =
        crate::db::topology::Topology::new(&config.data_dir).legacy_flat_content_db();
    if legacy_content_path.exists() {
        if let Ok(conn) = Connection::open(&legacy_content_path) {
            // URLs
            if let Ok(mut stmt) = conn.prepare("SELECT code FROM urls;") {
                if let Ok(mut rows) = stmt.query([]) {
                    while let Some(row) = rows.next().unwrap_or(None) {
                        if let Ok(code) = row.get::<_, String>(0) {
                            slug_owners.entry(code).or_default().push("1".to_string());
                        }
                    }
                }
            }
            // Landing Pages
            if let Ok(mut stmt) = conn.prepare("SELECT code FROM landing_pages;") {
                if let Ok(mut rows) = stmt.query([]) {
                    while let Some(row) = rows.next().unwrap_or(None) {
                        if let Ok(code) = row.get::<_, String>(0) {
                            slug_owners.entry(code).or_default().push("1".to_string());
                        }
                    }
                }
            }
        }
    }

    // 2. Scan all tenant databases in data_dir/users/<id>/content.db
    let users_dir = crate::db::topology::Topology::new(&config.data_dir).users_dir();
    if users_dir.exists() {
        for entry in std::fs::read_dir(users_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                if let Some(name_str) = path.file_name().and_then(|n| n.to_str()) {
                    if crate::db::topology::is_valid_user_dir_name(name_str) {
                        let content_db_path = path.join("content.db");
                        if content_db_path.exists() {
                            if let Ok(conn) = Connection::open(&content_db_path) {
                                // URLs
                                if let Ok(mut stmt) = conn.prepare("SELECT code FROM urls;") {
                                    if let Ok(mut rows) = stmt.query([]) {
                                        while let Some(row) = rows.next().unwrap_or(None) {
                                            if let Ok(code) = row.get::<_, String>(0) {
                                                slug_owners
                                                    .entry(code)
                                                    .or_default()
                                                    .push(name_str.to_string());
                                            }
                                        }
                                    }
                                }
                                // Landing pages
                                if let Ok(mut stmt) =
                                    conn.prepare("SELECT code FROM landing_pages;")
                                {
                                    if let Ok(mut rows) = stmt.query([]) {
                                        while let Some(row) = rows.next().unwrap_or(None) {
                                            if let Ok(code) = row.get::<_, String>(0) {
                                                slug_owners
                                                    .entry(code)
                                                    .or_default()
                                                    .push(name_str.to_string());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 3. Populate report
    for (slug, owners) in slug_owners {
        if owners.len() > 1 {
            duplicates.push(format!(
                "Slug '{}' is defined in multiple content databases by owners {:?}",
                slug, owners
            ));
        }
        // Validate slug format
        let valid_url = crate::utils::validation::validate_redirect_code(&slug);
        let valid_page = crate::utils::validation::validate_page_code(&slug);
        if !valid_url && !valid_page {
            invalid_entries.push(format!("Slug '{}' is format-invalid", slug));
        }
    }

    Ok(SlugAuditReport {
        duplicates,
        invalid_entries,
        warnings,
    })
}

#[deprecated(
    note = "Legacy v0.7 global_slugs function; use crate::db::slugs::cleanup_stale_reservations instead"
)]
pub fn cleanup_stale_reservations(
    system_conn: &Connection,
    data_dir: &std::path::Path,
) -> Result<usize, Box<dyn std::error::Error>> {
    use chrono::{DateTime, Utc};
    let mut cleaned_count = 0;

    let mut stmt = system_conn.prepare(
        "SELECT slug, owner_user_id, target_type, created_at FROM global_slugs WHERE status = 'reserving';"
    )?;
    let mut rows = stmt.query([])?;
    let mut stale_slugs = Vec::new();

    while let Some(row) = rows.next()? {
        let slug: String = row.get(0)?;
        let owner_user_id: i64 = row.get(1)?;
        let target_type: String = row.get(2)?;
        let created_at_str: String = row.get(3)?;

        if let Ok(created_at) = DateTime::parse_from_rfc3339(&created_at_str) {
            let age = Utc::now().signed_duration_since(created_at.with_timezone(&Utc));
            if age > chrono::Duration::minutes(15) {
                // Check if target record exists by looking up code = slug in owner's content.db
                let topology = crate::db::topology::Topology::new(data_dir);
                let content_db_path = {
                    let users_path = topology.users_registry_db();
                    if let Ok(users_conn) = Connection::open(&users_path) {
                        crate::db::tenant::existing_content_path(
                            &users_conn,
                            &topology,
                            owner_user_id,
                        )
                        .ok()
                        .or_else(|| {
                            if owner_user_id == 1 {
                                Some(topology.legacy_flat_content_db())
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(|| topology.legacy_flat_content_db())
                    } else if owner_user_id == 1 {
                        topology.legacy_flat_content_db()
                    } else {
                        topology
                            .content_db_i64(owner_user_id)
                            .unwrap_or_else(|_| topology.legacy_flat_content_db())
                    }
                };

                let mut target_exists = false;
                if content_db_path.exists() {
                    if let Ok(conn) = Connection::open(&content_db_path) {
                        if target_type == "url" {
                            target_exists = conn
                                .query_row(
                                    "SELECT EXISTS(SELECT 1 FROM urls WHERE code = ?1);",
                                    [&slug],
                                    |r| r.get(0),
                                )
                                .unwrap_or(false);
                        } else if target_type == "page" {
                            target_exists = conn
                                .query_row(
                                    "SELECT EXISTS(SELECT 1 FROM landing_pages WHERE code = ?1);",
                                    [&slug],
                                    |r| r.get(0),
                                )
                                .unwrap_or(false);
                        }
                    }
                }

                if !target_exists {
                    stale_slugs.push((slug, owner_user_id));
                }
            }
        }
    }

    drop(rows);
    drop(stmt);

    for (slug, owner_user_id) in stale_slugs {
        system_conn.execute("DELETE FROM global_slugs WHERE slug = ?1;", [&slug])?;
        let now = Utc::now().to_rfc3339();
        system_conn.execute(
            "INSERT INTO slug_history (slug, old_owner_user_id, new_owner_user_id, action, timestamp)
             VALUES (?1, ?2, NULL, 'released', ?3);",
            rusqlite::params![slug, owner_user_id, now],
        )?;
        cleaned_count += 1;
    }

    Ok(cleaned_count)
}

#[deprecated(
    note = "Legacy v0.7 global_slugs function; use v0.8 slug databases and TenantId instead"
)]
pub fn register_restored_user_slugs(
    system_conn: &Connection,
    target_user_id: i64,
    restored_content_db_path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let restored_content_conn = Connection::open(restored_content_db_path)?;

    let mut urls = Vec::new();
    let mut landing_pages = Vec::new();

    // 1. Read URLs
    {
        let mut stmt =
            restored_content_conn.prepare("SELECT code, id, created_at, status FROM urls;")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let code: String = row.get(0)?;
            let id: String = row.get(1)?;
            let created_at: String = row.get(2)?;
            let status: String = row.get(3)?;
            urls.push((code, id, created_at, status));
        }
    }

    // 2. Read Landing Pages
    {
        let mut stmt = restored_content_conn
            .prepare("SELECT code, id, created_at, state FROM landing_pages;")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let code: String = row.get(0)?;
            let id: String = row.get(1)?;
            let created_at: String = row.get(2)?;
            let state: String = row.get(3)?;
            landing_pages.push((code, id, created_at, state));
        }
    }

    // 3. Check for collisions across all URLs and landing pages
    let mut conflicting_slugs = Vec::new();
    for (slug, _, _, _) in &urls {
        let existing_owner: Option<i64> = system_conn
            .query_row(
                "SELECT owner_user_id FROM global_slugs WHERE slug = ?1;",
                [slug],
                |r| r.get(0),
            )
            .optional()?;

        if let Some(owner) = existing_owner {
            if owner != target_user_id {
                conflicting_slugs.push(slug.clone());
            }
        }
    }

    for (slug, _, _, _) in &landing_pages {
        let existing_owner: Option<i64> = system_conn
            .query_row(
                "SELECT owner_user_id FROM global_slugs WHERE slug = ?1;",
                [slug],
                |r| r.get(0),
            )
            .optional()?;

        if let Some(owner) = existing_owner {
            if owner != target_user_id {
                conflicting_slugs.push(slug.clone());
            }
        }
    }

    if !conflicting_slugs.is_empty() {
        return Err(format!(
            "Restore failed. Conflicting slugs: {}",
            conflicting_slugs.join(", ")
        )
        .into());
    }

    // 4. Perform registration
    system_conn.execute(
        "DELETE FROM global_slugs WHERE owner_user_id = ?1;",
        [target_user_id],
    )?;

    for (slug, target_id, created_at, status) in urls {
        let now = Utc::now().to_rfc3339();
        let global_status = if status == "dead" {
            "disabled"
        } else {
            "active"
        };
        system_conn.execute(
            "INSERT OR REPLACE INTO global_slugs (slug, owner_user_id, target_type, target_id, created_at, updated_at, status) 
             VALUES (?1, ?2, 'url', ?3, ?4, ?5, ?6);",
            rusqlite::params![slug, target_user_id, target_id, created_at, now, global_status],
        )?;
    }

    for (slug, target_id, created_at, state) in landing_pages {
        let now = Utc::now().to_rfc3339();
        let status = if state == "published" {
            "active"
        } else {
            "disabled"
        };
        system_conn.execute(
            "INSERT OR REPLACE INTO global_slugs (slug, owner_user_id, target_type, target_id, created_at, updated_at, status) 
             VALUES (?1, ?2, 'page', ?3, ?4, ?5, ?6);",
            rusqlite::params![slug, target_user_id, target_id, created_at, now, status],
        )?;
    }

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

/// Calculate aggregate platform total clicks across all active tenant analytics databases.
pub fn get_platform_total_clicks(
    topology: &crate::db::topology::Topology,
    users_conn: &Connection,
) -> Option<i64> {
    let mut stmt = users_conn
        .prepare("SELECT tenant_id FROM users WHERE status != 'deleted' AND tenant_id IS NOT NULL;")
        .ok()?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0)).ok()?;
    let mut total = 0i64;
    for tid_str in rows.flatten() {
        if let Ok(tid) = crate::identity::TenantId::parse(&tid_str) {
            let analytics_path = topology.tenant_analytics_db(tid);
            if analytics_path.exists() {
                if let Ok(conn) = Connection::open(&analytics_path) {
                    let count: i64 = conn
                        .query_row("SELECT COUNT(*) FROM visits;", [], |r| r.get(0))
                        .unwrap_or(0);
                    total += count;
                }
            }
        }
    }
    Some(total)
}
