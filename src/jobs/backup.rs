use super::{log_job_end, log_job_start};
use crate::config::Config;
use crate::db::Db;
use std::time::Duration;
use tracing::{error, info};

pub async fn run_backup_scheduler(db: Db, config: Config) {
    if !config.backup_enabled {
        info!("Background backup scheduler is disabled.");
        return;
    }

    info!(
        "Starting background backup scheduler (interval: {} mins)...",
        config.backup_interval_mins
    );
    loop {
        // Run backup every configured interval
        tokio::time::sleep(Duration::from_secs(config.backup_interval_mins * 60)).await;
        info!("Running background database backup...");

        let job_id = log_job_start(&db.system, "database_backup");
        match perform_backup(&db, &config).await {
            Ok(path) => {
                info!("Backup created successfully at {}", path);
                log_job_end(&db.system, &job_id, "success", None);
            }
            Err(e) => {
                let err_str = e.to_string();
                error!("Error performing backup: {}", err_str);
                log_job_end(&db.system, &job_id, "failed", Some(&err_str));
            }
        }
    }
}

pub async fn perform_backup(
    db: &Db,
    config: &Config,
) -> Result<String, Box<dyn std::error::Error>> {
    use chrono::Utc;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use rusqlite::params;
    use std::fs::File;
    use tar::Builder;
    use uuid::Uuid;

    let out_dir = config.backup_dir.clone();
    if !out_dir.exists() {
        std::fs::create_dir_all(&out_dir)?;
    }

    // Force checkpoint on all databases to flush WAL contents to the main DB files
    if let Ok(conn) = db.admin.lock() {
        let _ = conn.execute("PRAGMA wal_checkpoint(TRUNCATE);", []);
    }
    if let Ok(conn) = db.content.lock() {
        let _ = conn.execute("PRAGMA wal_checkpoint(TRUNCATE);", []);
    }
    if let Ok(conn) = db.analytics.lock() {
        let _ = conn.execute("PRAGMA wal_checkpoint(TRUNCATE);", []);
    }
    if let Ok(conn) = db.system.lock() {
        let _ = conn.execute("PRAGMA wal_checkpoint(TRUNCATE);", []);
    }

    let date_str = Utc::now().format("%Y-%m-%d-%H%M%S").to_string();
    let tar_name = format!("{}-bzod-backup.tar.gz", date_str);
    let tar_path = out_dir.join(tar_name);

    let file = File::create(&tar_path)?;
    let enc = GzEncoder::new(file, Compression::default());
    let mut tar = Builder::new(enc);

    let files = vec!["admin.db", "content.db", "analytics.db", "system.db"];
    for f in files {
        let db_file = config.data_dir.join(f);
        if db_file.exists() {
            tar.append_path_with_name(&db_file, f)?;
        }
    }

    tar.into_inner()?.finish()?;
    let size_bytes = std::fs::metadata(&tar_path)?.len();
    let path_str = tar_path.to_string_lossy().to_string();

    // Log to system.db.backup_history
    {
        let conn = db.system.lock().unwrap();
        let backup_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let _ = conn.execute(
            "INSERT INTO backup_history (id, backup_path, status, created_at, size_bytes, error_message) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6);",
            params![backup_id, path_str, "success", now, size_bytes as i64, None::<String>],
        );
    }

    Ok(path_str)
}
