use crate::config::Config;
use crate::db::Db;
use chrono::Utc;
use rusqlite::OptionalExtension;
use std::fs::File;
use std::path::PathBuf;
use tar::Archive;
use tracing::{error, info};
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

    // 2. Resolve target user ID and upsert user record in users.db
    let target_user_id = {
        let users_conn = db.users.lock().unwrap();
        let existing_user =
            crate::db::users::get_user_by_username(&users_conn, &metadata.username)?;

        match existing_user {
            Some(u) => {
                users_conn.execute(
                    "UPDATE users SET password_hash = ?1, status = ?2, account_type = ?3, metadata = ?4 WHERE id = ?5;",
                    rusqlite::params![metadata.password_hash, metadata.status, metadata.account_type, metadata.metadata, u.id],
                )?;
                users_conn.execute(
                    "INSERT OR REPLACE INTO quotas (user_id, max_urls, max_landings, max_api_tokens, max_storage_mb) 
                     VALUES (?1, ?2, ?3, ?4, ?5);",
                    rusqlite::params![u.id, metadata.quotas.max_urls, metadata.quotas.max_landings, metadata.quotas.max_api_tokens, metadata.quotas.max_storage_mb],
                )?;
                u.id
            }
            None => {
                let id_taken: bool = users_conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM users WHERE id = ?1);",
                        [metadata.id],
                        |row| row.get(0),
                    )
                    .unwrap_or(false);

                let new_id = if !id_taken {
                    users_conn.execute(
                        "INSERT INTO users (id, username, password_hash, status, created_at, account_type, metadata) 
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);",
                        rusqlite::params![metadata.id, metadata.username, metadata.password_hash, metadata.status, metadata.created_at, metadata.account_type, metadata.metadata],
                    )?;
                    metadata.id
                } else {
                    users_conn.execute(
                        "INSERT INTO users (username, password_hash, status, created_at, account_type, metadata) 
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6);",
                        rusqlite::params![metadata.username, metadata.password_hash, metadata.status, metadata.created_at, metadata.account_type, metadata.metadata],
                    )?;
                    users_conn.last_insert_rowid()
                };

                users_conn.execute(
                    "INSERT OR REPLACE INTO quotas (user_id, max_urls, max_landings, max_api_tokens, max_storage_mb) 
                     VALUES (?1, ?2, ?3, ?4, ?5);",
                    rusqlite::params![new_id, metadata.quotas.max_urls, metadata.quotas.max_landings, metadata.quotas.max_api_tokens, metadata.quotas.max_storage_mb],
                )?;
                new_id
            }
        }
    };

    // 3. Extract database files to /data/users/<target_user_id>/
    let dest_dir = config
        .data_dir
        .join("users")
        .join(target_user_id.to_string());
    std::fs::create_dir_all(&dest_dir)?;

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

    // 4. Register slugs in global_slugs
    let restored_content_conn = rusqlite::Connection::open(dest_dir.join("content.db"))?;
    {
        let mut system_conn = db.system.lock().unwrap();
        let tx = system_conn.transaction()?;

        // Delete any existing global slugs owned by this user
        tx.execute(
            "DELETE FROM global_slugs WHERE owner_user_id = ?1;",
            [target_user_id],
        )?;

        // Register URLs
        {
            let mut stmt =
                restored_content_conn.prepare("SELECT code, id, created_at, status FROM urls;")?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let slug: String = row.get(0)?;
                let target_id: String = row.get(1)?;
                let created_at: String = row.get(2)?;
                let status: String = row.get(3)?;
                let now = Utc::now().to_rfc3339();

                let existing_owner: Option<i64> = tx
                    .query_row(
                        "SELECT owner_user_id FROM global_slugs WHERE slug = ?1;",
                        [&slug],
                        |r| r.get(0),
                    )
                    .optional()?;

                if let Some(owner) = existing_owner {
                    if owner != target_user_id {
                        error!(
                            "Conflict: Slug '{}' is already owned by user ID {}. Skipping.",
                            slug, owner
                        );
                        continue;
                    }
                }

                tx.execute(
                    "INSERT OR REPLACE INTO global_slugs (slug, owner_user_id, target_type, target_id, created_at, updated_at, status) 
                     VALUES (?1, ?2, 'url', ?3, ?4, ?5, ?6);",
                    rusqlite::params![slug, target_user_id, target_id, created_at, now, status],
                )?;
            }
        }

        // Register Landing Pages
        {
            let mut stmt = restored_content_conn
                .prepare("SELECT code, id, created_at, state FROM landing_pages;")?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let slug: String = row.get(0)?;
                let target_id: String = row.get(1)?;
                let created_at: String = row.get(2)?;
                let state: String = row.get(3)?;
                let now = Utc::now().to_rfc3339();
                let status = if state == "published" {
                    "active"
                } else {
                    "disabled"
                };

                let existing_owner: Option<i64> = tx
                    .query_row(
                        "SELECT owner_user_id FROM global_slugs WHERE slug = ?1;",
                        [&slug],
                        |r| r.get(0),
                    )
                    .optional()?;

                if let Some(owner) = existing_owner {
                    if owner != target_user_id {
                        error!(
                            "Conflict: Slug '{}' is already owned by user ID {}. Skipping.",
                            slug, owner
                        );
                        continue;
                    }
                }

                tx.execute(
                    "INSERT OR REPLACE INTO global_slugs (slug, owner_user_id, target_type, target_id, created_at, updated_at, status) 
                     VALUES (?1, ?2, 'page', ?3, ?4, ?5, ?6);",
                    rusqlite::params![slug, target_user_id, target_id, created_at, now, status],
                )?;
            }
        }

        tx.commit()?;
    }

    // 5. Reconcile quotas for restored user
    crate::db::users::reconcile_user_quotas(
        &db.users.lock().unwrap(),
        target_user_id,
        &restored_content_conn,
    )?;

    info!(
        "User '{}' (ID: {}) successfully restored from backup.",
        metadata.username, target_user_id
    );
    Ok(())
}
