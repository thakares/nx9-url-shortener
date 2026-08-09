use super::{log_job_end, log_job_start};
use crate::config::Config;
use crate::db::Db;
use std::time::Duration;
use tracing::{error, info};

pub async fn run_backup_scheduler(
    db: Db,
    config: Config,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
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
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(config.backup_interval_mins * 60)) => {}
            _ = shutdown_rx.changed() => {
                info!("Backup scheduler shutting down...");
                break;
            }
        }
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
    if let Ok(conn) = db.users.lock() {
        let _ = conn.execute("PRAGMA wal_checkpoint(TRUNCATE);", []);
        if let Ok(mut stmt) = conn.prepare("SELECT id FROM users;") {
            if let Ok(rows) = stmt.query_map([], |row| row.get::<_, i64>(0)) {
                let user_ids: Vec<i64> = rows.filter_map(|r| r.ok()).collect();
                for user_id in user_ids {
                    if let Ok(u_conn) = crate::jobs::open_user_content_conn(db, user_id) {
                        let _ = u_conn.execute("PRAGMA wal_checkpoint(TRUNCATE);", []);
                    }
                    if let Ok(u_conn) = crate::jobs::open_user_analytics_conn(db, user_id) {
                        let _ = u_conn.execute("PRAGMA wal_checkpoint(TRUNCATE);", []);
                    }
                }
            }
        }
    }

    let date_str = Utc::now().format("%Y-%m-%d-%H%M%S").to_string();
    let tar_name = format!("{}-bzod-backup.tar.gz", date_str);
    let tar_path = out_dir.join(tar_name);

    let file = File::create(&tar_path)?;
    let enc = GzEncoder::new(file, Compression::default());
    let mut tar = Builder::new(enc);

    let admin_dir = config.data_dir.join("admin");
    if admin_dir.exists() {
        tar.append_dir_all("admin", &admin_dir)?;
    }

    let users_dir = config.data_dir.join("users");
    if users_dir.exists() {
        tar.append_dir_all("users", &users_dir)?;
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
