use crate::config::Config;
use crate::db::Db;
use chrono::Utc;
use std::fs::File;
use std::path::{Path, PathBuf};
use tar::{Builder, Header};
use tracing::{error, info};
use zstd::Encoder;

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
    username: String,
    out: Option<String>,
    data_dir: Option<String>,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(d) = data_dir {
        config.data_dir = PathBuf::from(d);
    }
    let db = Db::init(&config)?;

    let username_clean = username.trim().to_lowercase();

    // 1. Get user details from users.db
    let user_details = {
        let conn = db.users.lock().unwrap();
        crate::db::users::get_user_by_username(&conn, &username_clean)?
    };

    let user = match user_details {
        Some(u) => u,
        None => {
            error!("User '{}' not found", username_clean);
            return Ok(());
        }
    };

    let user_id = user.id;

    // 2. Fetch user's quotas
    let quotas = {
        let conn = db.users.lock().unwrap();
        conn.query_row(
            "SELECT max_urls, max_landings, max_api_tokens, max_storage_mb FROM quotas WHERE user_id = ?1;",
            [user_id],
            |row| {
                Ok(UserBackupQuotas {
                    max_urls: row.get(0)?,
                    max_landings: row.get(1)?,
                    max_api_tokens: row.get(2)?,
                    max_storage_mb: row.get(3)?,
                })
            }
        )?
    };

    // 3. Define output path
    let tar_path = match out {
        Some(p) => PathBuf::from(p),
        None => {
            if !config.backup_dir.exists() {
                std::fs::create_dir_all(&config.backup_dir)?;
            }
            config.backup_dir.join(format!(
                "{}-{}.tar.zst",
                username_clean,
                Utc::now().format("%Y%m%d")
            ))
        }
    };

    info!(
        "Backing up user {} (ID: {}) to {:?}",
        username_clean, user_id, tar_path
    );

    // 4. Force checkpoint on user's databases
    let user_dir = crate::db::tenant::location_for_user(&user)?.dir(&db.topology)?;
    if let Ok(c) = rusqlite::Connection::open(user_dir.join("content.db")) {
        let _ = c.execute("PRAGMA wal_checkpoint(TRUNCATE);", []);
    }
    if let Ok(c) = rusqlite::Connection::open(user_dir.join("analytics.db")) {
        let _ = c.execute("PRAGMA wal_checkpoint(TRUNCATE);", []);
    }
    if let Ok(c) = rusqlite::Connection::open(user_dir.join("profile.db")) {
        let _ = c.execute("PRAGMA wal_checkpoint(TRUNCATE);", []);
    }

    // 5. Create tar.zst archive
    let file = File::create(&tar_path)?;
    let zst_enc = Encoder::new(file, 3)?;
    let mut tar = Builder::new(zst_enc);

    // Write metadata.json directly into tar
    let metadata_obj = UserBackupMetadata {
        id: user.id,
        username: user.username,
        password_hash: user.password_hash,
        status: user.status,
        created_at: user.created_at,
        account_type: user.account_type,
        metadata: user.metadata,
        tenant_id: user.tenant_id.map(|t| t.to_string()),
        uuid: user.uuid,
        quotas,
    };
    let metadata_bytes = serde_json::to_vec_pretty(&metadata_obj)?;
    let mut header = Header::new_gnu();
    header.set_size(metadata_bytes.len() as u64);
    header.set_path("metadata.json")?;
    header.set_mode(0o644);
    header.set_cksum();
    tar.append(&header, &metadata_bytes[..])?;

    // Append database files
    let mut append_file =
        |name_in_archive: &str, path_on_disk: &Path| -> Result<(), Box<dyn std::error::Error>> {
            if path_on_disk.exists() {
                let mut file = File::open(path_on_disk)?;
                tar.append_file(name_in_archive, &mut file)?;
            }
            Ok(())
        };

    append_file("content.db", &user_dir.join("content.db"))?;
    append_file("analytics.db", &user_dir.join("analytics.db"))?;
    append_file("profile.db", &user_dir.join("profile.db"))?;

    tar.into_inner()?.finish()?;

    info!("User backup generated successfully at {:?}", tar_path);
    Ok(())
}
