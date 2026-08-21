use crate::config::Config;
use crate::db::Db;
use crate::identity::TenantId;
use chrono::Utc;
use rusqlite::OptionalExtension;
use std::fs::File;
use std::path::PathBuf;
use tar::Archive;
use tracing::{error, info};
use uuid::Uuid;
use zstd::Decoder;

#[derive(serde::Serialize, serde::Deserialize)]
struct UserBackupMetadata {
    id: i64,
    username: String,
    password_hash: String,
    status: String,
    created_at: String,
    account_type: String,
    metadata: Option<String>,
    #[serde(default)]
    tenant_id: Option<String>,
    #[serde(default)]
    uuid: Option<String>,
    quotas: UserBackupQuotas,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct UserBackupQuotas {
    max_urls: i64,
    max_landings: i64,
    max_api_tokens: i64,
    max_storage_mb: i64,
}

pub async fn run(
    file: String,
    data_dir: Option<String>,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(d) = data_dir {
        config.data_dir = PathBuf::from(d);
    }
    let file_path = PathBuf::from(file);

    if !file_path.exists() {
        error!("Backup file not found: {:?}", file_path);
        return Ok(());
    }

    let db = Db::init(&config)?;

    // 1. Read metadata.json from the tar.zst archive
    let f = File::open(&file_path)?;
    let zst_dec = Decoder::new(f)?;
    let mut archive = Archive::new(zst_dec);

    let mut metadata_opt: Option<UserBackupMetadata> = None;
    for entry_res in archive.entries()? {
        let mut entry = entry_res?;
        let path = entry.path()?;
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if file_name == "metadata.json" {
            let meta: UserBackupMetadata = serde_json::from_reader(&mut entry)?;
            metadata_opt = Some(meta);
            break;
        }
    }

    let metadata = match metadata_opt {
        Some(m) => m,
        None => {
            error!("Archive is missing metadata.json");
            return Ok(());
        }
    };

    info!("Restoring user {} from backup...", metadata.username);

    // 2. Resolve identity per Correction #1:
    // Order:
    // 1. Archive contains TenantId + UUID -> preserve exactly.
    // 2. Legacy archive matched to existing users.db account -> resolve and preserve that user's existing TenantId + UUID.
    // 3. Genuinely new legacy restore with no existing identity match -> explicitly allocate new TenantId + UUID.
    let (target_user_id, target_tenant_id) = {
        let users_conn = db.users.lock().unwrap();
        let existing_user =
            crate::db::users::get_user_by_username(&users_conn, &metadata.username)?;

        match existing_user {
            Some(u) => {
                let tenant_id = if let Some(tid) = u.tenant_id {
                    tid
                } else if let Some(ref tid_str) = metadata.tenant_id {
                    TenantId::parse(tid_str).unwrap_or_else(|_| TenantId::generate())
                } else {
                    TenantId::generate()
                };

                let user_uuid = if let Some(ref uid) = u.uuid {
                    uid.clone()
                } else if let Some(ref uid_str) = metadata.uuid {
                    uid_str.clone()
                } else {
                    Uuid::new_v4().to_string()
                };

                users_conn.execute(
                    "UPDATE users SET password_hash = ?1, status = ?2, account_type = ?3, metadata = ?4, tenant_id = ?5, uuid = ?6 WHERE id = ?7;",
                    rusqlite::params![
                        metadata.password_hash,
                        metadata.status,
                        metadata.account_type,
                        metadata.metadata,
                        tenant_id.as_str(),
                        user_uuid,
                        u.id
                    ],
                )?;
                users_conn.execute(
                    "INSERT OR REPLACE INTO quotas (user_id, max_urls, max_landings, max_api_tokens, max_storage_mb) 
                     VALUES (?1, ?2, ?3, ?4, ?5);",
                    rusqlite::params![
                        u.id,
                        metadata.quotas.max_urls,
                        metadata.quotas.max_landings,
                        metadata.quotas.max_api_tokens,
                        metadata.quotas.max_storage_mb
                    ],
                )?;
                (u.id, tenant_id)
            }
            None => {
                let tenant_id = if let Some(ref tid_str) = metadata.tenant_id {
                    TenantId::parse(tid_str).unwrap_or_else(|_| TenantId::generate())
                } else {
                    TenantId::generate()
                };

                let user_uuid = if let Some(ref uid_str) = metadata.uuid {
                    uid_str.clone()
                } else {
                    Uuid::new_v4().to_string()
                };

                let id_taken: bool = users_conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM users WHERE id = ?1);",
                        [metadata.id],
                        |row| row.get(0),
                    )
                    .unwrap_or(false);

                let new_id = if !id_taken {
                    users_conn.execute(
                        "INSERT INTO users (id, username, password_hash, status, created_at, account_type, metadata, tenant_id, uuid) 
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9);",
                        rusqlite::params![
                            metadata.id,
                            metadata.username,
                            metadata.password_hash,
                            metadata.status,
                            metadata.created_at,
                            metadata.account_type,
                            metadata.metadata,
                            tenant_id.as_str(),
                            user_uuid
                        ],
                    )?;
                    metadata.id
                } else {
                    users_conn.execute(
                        "INSERT INTO users (username, password_hash, status, created_at, account_type, metadata, tenant_id, uuid) 
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8);",
                        rusqlite::params![
                            metadata.username,
                            metadata.password_hash,
                            metadata.status,
                            metadata.created_at,
                            metadata.account_type,
                            metadata.metadata,
                            tenant_id.as_str(),
                            user_uuid
                        ],
                    )?;
                    users_conn.last_insert_rowid()
                };

                users_conn.execute(
                    "INSERT OR REPLACE INTO quotas (user_id, max_urls, max_landings, max_api_tokens, max_storage_mb) 
                     VALUES (?1, ?2, ?3, ?4, ?5);",
                    rusqlite::params![
                        new_id,
                        metadata.quotas.max_urls,
                        metadata.quotas.max_landings,
                        metadata.quotas.max_api_tokens,
                        metadata.quotas.max_storage_mb
                    ],
                )?;
                (new_id, tenant_id)
            }
        }
    };

    // 3. Extract database files to /data/users/<TenantId>/
    let dest_dir = db.topology.tenant_dir(target_tenant_id);
    std::fs::create_dir_all(&dest_dir)?;
    std::fs::create_dir_all(dest_dir.join("extensions"))?;

    let f2 = File::open(&file_path)?;
    let zst_dec2 = Decoder::new(f2)?;
    let mut archive2 = Archive::new(zst_dec2);

    for entry_res in archive2.entries()? {
        let mut entry = entry_res?;
        let path = entry.path()?;
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        match file_name {
            "content.db" => {
                let mut out_file = File::create(dest_dir.join("content.db"))?;
                std::io::copy(&mut entry, &mut out_file)?;
            }
            "analytics.db" => {
                let mut out_file = File::create(dest_dir.join("analytics.db"))?;
                std::io::copy(&mut entry, &mut out_file)?;
            }
            "profile.db" => {
                let mut out_file = File::create(dest_dir.join("profile.db"))?;
                std::io::copy(&mut entry, &mut out_file)?;
            }
            _ => {}
        }
    }

    // 4. Register restored slugs in v0.8 slug databases (global_urls.db, global_landing_pages.db)
    let content_path = dest_dir.join("content.db");
    if content_path.exists() {
        let restored_content_conn = rusqlite::Connection::open(&content_path)?;
        let now = Utc::now().to_rfc3339();

        let urls_conn = db.global_urls.lock().unwrap();
        let pages_conn = db.global_landing_pages.lock().unwrap();
        let reserved_conn = db.reserved.lock().unwrap();

        // 4a. Validate URL slugs for collisions
        let mut url_slugs = Vec::new();
        let mut stmt =
            restored_content_conn.prepare("SELECT code, id, created_at, status FROM urls;")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let code: String = row.get(0)?;
            let target_id: String = row.get(1)?;
            let created_at: String = row.get(2)?;
            let status: String = row.get(3)?;
            let global_status = if status == "dead" {
                "disabled"
            } else {
                "active"
            };

            // Check collision with global_urls
            let existing_url_owner: Option<String> = urls_conn
                .query_row(
                    "SELECT owner_tenant_id FROM global_urls WHERE slug = ?1;",
                    [&code],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(ref owner) = existing_url_owner {
                if owner != target_tenant_id.as_str() {
                    return Err(format!(
                        "Slug collision: URL slug '{code}' is already registered to another tenant ({owner})"
                    )
                    .into());
                }
            }

            // Check collision with global_landing_pages
            let existing_page_owner: Option<String> = pages_conn
                .query_row(
                    "SELECT owner_tenant_id FROM global_landing_pages WHERE slug = ?1;",
                    [&code],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(ref owner) = existing_page_owner {
                if owner != target_tenant_id.as_str() {
                    return Err(format!(
                        "Slug collision: URL slug '{code}' collides with landing page owned by tenant ({owner})"
                    )
                    .into());
                }
            }

            // Check reserved
            let is_reserved: bool = reserved_conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM reserved_slugs WHERE slug = ?1);",
                    [&code],
                    |r| r.get(0),
                )
                .unwrap_or(false);
            if is_reserved {
                return Err(format!(
                    "Slug collision: URL slug '{code}' is a reserved system keyword"
                )
                .into());
            }

            url_slugs.push((code, target_id, created_at, global_status));
        }

        // 4b. Validate Landing Page slugs for collisions
        let mut page_slugs = Vec::new();
        let mut stmt = restored_content_conn
            .prepare("SELECT code, id, created_at, state FROM landing_pages;")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let code: String = row.get(0)?;
            let target_id: String = row.get(1)?;
            let created_at: String = row.get(2)?;
            let state: String = row.get(3)?;
            let global_status = if state == "published" {
                "active"
            } else {
                "disabled"
            };

            let existing_url_owner: Option<String> = urls_conn
                .query_row(
                    "SELECT owner_tenant_id FROM global_urls WHERE slug = ?1;",
                    [&code],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(ref owner) = existing_url_owner {
                if owner != target_tenant_id.as_str() {
                    return Err(format!(
                        "Slug collision: Landing page slug '{code}' collides with URL owned by tenant ({owner})"
                    )
                    .into());
                }
            }

            let existing_page_owner: Option<String> = pages_conn
                .query_row(
                    "SELECT owner_tenant_id FROM global_landing_pages WHERE slug = ?1;",
                    [&code],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(ref owner) = existing_page_owner {
                if owner != target_tenant_id.as_str() {
                    return Err(format!(
                        "Slug collision: Landing page slug '{code}' is already registered to another tenant ({owner})"
                    )
                    .into());
                }
            }

            let is_reserved: bool = reserved_conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM reserved_slugs WHERE slug = ?1);",
                    [&code],
                    |r| r.get(0),
                )
                .unwrap_or(false);
            if is_reserved {
                return Err(format!(
                    "Slug collision: Landing page slug '{code}' is a reserved system keyword"
                )
                .into());
            }

            page_slugs.push((code, target_id, created_at, global_status));
        }

        // 4c. Register validated URL slugs
        for (code, target_id, created_at, global_status) in url_slugs {
            let _ = urls_conn.execute(
                "INSERT OR REPLACE INTO global_urls (slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at) 
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL);",
                rusqlite::params![
                    code,
                    target_tenant_id.as_str(),
                    target_id,
                    created_at,
                    now,
                    global_status
                ],
            );
        }

        // 4d. Register validated Landing Page slugs
        for (code, target_id, created_at, global_status) in page_slugs {
            let _ = pages_conn.execute(
                "INSERT OR REPLACE INTO global_landing_pages (slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at) 
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL);",
                rusqlite::params![
                    code,
                    target_tenant_id.as_str(),
                    target_id,
                    created_at,
                    now,
                    global_status
                ],
            );
        }

        // 5. Reconcile quotas for restored user
        crate::db::users::reconcile_user_quotas(
            &db.users.lock().unwrap(),
            target_user_id,
            &restored_content_conn,
        )?;
    }

    info!(
        "User '{}' (ID: {}, Tenant: {}) successfully restored from backup.",
        metadata.username, target_user_id, target_tenant_id
    );
    Ok(())
}
