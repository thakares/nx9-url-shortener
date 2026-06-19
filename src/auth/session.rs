use crate::db::admin::{
    get_api_key_by_hash, get_user_by_id as get_admin_user_by_id, update_api_key_last_used,
};
use crate::models::{ApiActor, Session as AdminSession, TenantUser, User, UserSession};
use axum_extra::extract::CookieJar;
use chrono::Utc;
use rand::{thread_rng, RngCore};
use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};

// Generate a secure random token (hex-encoded)
pub fn generate_token(bytes_len: usize) -> String {
    let mut key = vec![0u8; bytes_len];
    thread_rng().fill_bytes(&mut key);
    hex::encode(key)
}

// Authenticate administrator session from cookies
pub fn authenticate_admin_session(
    conn: &Connection,
    jar: &CookieJar,
) -> Result<Option<(User, String)>, rusqlite::Error> {
    let cookie = match jar.get("bzod_session") {
        Some(c) => c,
        None => return Ok(None),
    };

    let session_id = cookie.value();
    let session_opt: Option<AdminSession> = conn
        .query_row(
            "SELECT id, user_id, expires_at, created_at FROM sessions WHERE id = ?1;",
            [session_id],
            |row| {
                let id: String = row.get(0)?;
                // `user_id` may be stored as integer (users.db) or text (admin.db UUID).
                let user_id_str: String = match row.get::<_, String>(1) {
                    Ok(s) => s,
                    Err(_) => {
                        let i: i64 = row.get(1)?;
                        i.to_string()
                    }
                };
                let expires_at: String = row.get(2)?;
                let created_at: String = row.get(3)?;
                Ok(AdminSession {
                    id,
                    user_id: user_id_str,
                    expires_at,
                    created_at,
                })
            },
        )
        .optional()?;

    let session = match session_opt {
        Some(s) => s,
        None => return Ok(None),
    };

    // Check expiration
    if let Ok(expires) = chrono::DateTime::parse_from_rfc3339(&session.expires_at) {
        if expires.with_timezone(&Utc) < Utc::now() {
            // Expired
            return Ok(None);
        }
    } else {
        return Ok(None);
    }

    // Get user (status must be 'active' and account_type = 'admin')
    // If the session user_id looks numeric, bind as integer when querying users.db
    // Try the extended lookup but catch errors (e.g., missing columns in legacy admin DB)
    // Try the extended lookup; if it errors (legacy schema), perform a fallback lookup.
    let (user_opt, extended_failed) = match if let Ok(id_i64) = session.user_id.parse::<i64>() {
        conn.query_row(
            "SELECT id, username, password_hash, created_at 
             FROM users WHERE id = ?1 AND status = 'active' AND account_type = 'admin';",
            [id_i64],
            |row| {
                let id_str = row.get::<_, i64>(0)?.to_string();
                Ok(User {
                    id: id_str,
                    username: row.get(1)?,
                    password_hash: row.get(2)?,
                    created_at: row.get(3)?,
                })
            },
        )
        .optional()
    } else {
        conn.query_row(
            "SELECT id, username, password_hash, created_at 
             FROM users WHERE id = ?1 AND status = 'active' AND account_type = 'admin';",
            [session.user_id.as_str()],
            |row| {
                // admin DB stores UUID string ids, so read as String
                let id_str: String = row.get(0)?;
                Ok(User {
                    id: id_str,
                    username: row.get(1)?,
                    password_hash: row.get(2)?,
                    created_at: row.get(3)?,
                })
            },
        )
        .optional()
    } {
        Ok(opt) => (opt, false),
        Err(_) => (None, true),
    };

    if let Some(user) = user_opt {
        return Ok(Some((user, session.id)));
    }

    if extended_failed {
        // Fallback for legacy admin.db schemas which may not have `status`/`account_type` columns
        // Try a simpler lookup by id only.
        let fallback_user_opt = if let Ok(id_i64) = session.user_id.parse::<i64>() {
            conn.query_row(
                "SELECT id, username, password_hash, created_at FROM users WHERE id = ?1;",
                [id_i64],
                |row| {
                    Ok(User {
                        id: row.get::<_, i64>(0)?.to_string(),
                        username: row.get(1)?,
                        password_hash: row.get(2)?,
                        created_at: row.get(3)?,
                    })
                },
            )
            .optional()
            .unwrap_or(None)
        } else {
            conn.query_row(
                "SELECT id, username, password_hash, created_at FROM users WHERE id = ?1;",
                [session.user_id.as_str()],
                |row| {
                    Ok(User {
                        id: row.get(0)?,
                        username: row.get(1)?,
                        password_hash: row.get(2)?,
                        created_at: row.get(3)?,
                    })
                },
            )
            .optional()
            .unwrap_or(None)
        };

        if let Some(user) = fallback_user_opt {
            Ok(Some((user, session.id)))
        } else {
            Ok(None)
        }
    } else {
        Ok(None)
    }
}

// Authenticate user session from cookies
pub fn authenticate_user_session(
    users_conn: &Connection,
    jar: &CookieJar,
) -> Result<Option<(TenantUser, String)>, rusqlite::Error> {
    let cookie = match jar.get("bzod_user_session") {
        Some(c) => c,
        None => return Ok(None),
    };

    let session_id = cookie.value();

    // Get session from sessions table in users.db
    let mut stmt = users_conn
        .prepare("SELECT id, user_id, expires_at, created_at FROM sessions WHERE id = ?1;")?;
    let session_opt: Option<UserSession> = stmt
        .query_row([session_id], |row| {
            Ok(UserSession {
                id: row.get(0)?,
                user_id: row.get(1)?,
                expires_at: row.get(2)?,
                created_at: row.get(3)?,
            })
        })
        .optional()?;

    let session = match session_opt {
        Some(s) => s,
        None => return Ok(None),
    };

    // Check expiration
    if let Ok(expires) = chrono::DateTime::parse_from_rfc3339(&session.expires_at) {
        if expires.with_timezone(&Utc) < Utc::now() {
            return Ok(None);
        }
    } else {
        return Ok(None);
    }

    // Get tenant user (status must be 'active')
    let mut stmt = users_conn.prepare(
        "SELECT id, username, password_hash, status, created_at, last_login, account_type, organization_id, metadata 
         FROM users WHERE id = ?1 AND status = 'active';"
    )?;
    let user_opt = stmt
        .query_row([session.user_id], |row| {
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
        })
        .optional()?;

    if let Some(user) = user_opt {
        Ok(Some((user, session.id)))
    } else {
        Ok(None)
    }
}

// Authenticate API key/token from Authorization header (unified)
pub fn authenticate_api_key(
    admin_conn: &Connection,
    users_conn: &Connection,
    auth_header: &str,
) -> Result<Option<ApiActor>, rusqlite::Error> {
    if !auth_header.starts_with("Bearer ") {
        return Ok(None);
    }

    let key = auth_header.trim_start_matches("Bearer ").trim();
    if key.is_empty() {
        return Ok(None);
    }

    // Hash the API key using SHA-256 to compare with stored hash
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    let hashed_key = hex::encode(hasher.finalize());

    // 1. Check user API tokens in users.db
    let mut stmt = users_conn.prepare("SELECT user_id FROM api_tokens WHERE token_hash = ?1;")?;
    let user_id_opt: Option<i64> = stmt.query_row([&hashed_key], |row| row.get(0)).optional()?;

    if let Some(user_id) = user_id_opt {
        let mut stmt = users_conn.prepare(
            "SELECT id, username, password_hash, status, created_at, last_login, account_type, organization_id, metadata 
             FROM users WHERE id = ?1 AND status = 'active';"
        )?;
        let user_opt = stmt
            .query_row([user_id], |row| {
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
            })
            .optional()?;

        if let Some(user) = user_opt {
            return Ok(Some(ApiActor::User(user)));
        }
    }

    // 2. Check admin system API keys in admin.db
    if let Some(api_key_rec) = get_api_key_by_hash(admin_conn, &hashed_key)? {
        // Update last used timestamp
        update_api_key_last_used(admin_conn, &api_key_rec.id)?;

        // Get admin user from users.db (users_conn)
        // Try to interpret the api_key user_id as an integer referencing users.db
        if let Ok(user_id_i64) = api_key_rec.user_id.parse::<i64>() {
            let mut stmt = users_conn.prepare(
                "SELECT id, username, password_hash, created_at 
                 FROM users WHERE id = ?1 AND status = 'active' AND account_type = 'admin';",
            )?;
            let user_opt = stmt
                .query_row([user_id_i64], |row| {
                    let id_i64: i64 = row.get(0)?;
                    Ok(User {
                        id: id_i64.to_string(),
                        username: row.get(1)?,
                        password_hash: row.get(2)?,
                        created_at: row.get(3)?,
                    })
                })
                .optional()?;

            if let Some(user) = user_opt {
                return Ok(Some(ApiActor::Admin(user)));
            }
        }

        // Fallback: admin DB may store users with string UUIDs. Try looking up directly in admin_conn.
        if let Ok(Some(admin_user)) = get_admin_user_by_id(admin_conn, &api_key_rec.user_id) {
            return Ok(Some(ApiActor::Admin(User {
                id: admin_user.id,
                username: admin_user.username,
                password_hash: admin_user.password_hash,
                created_at: admin_user.created_at,
            })));
        }
    }

    Ok(None)
}
