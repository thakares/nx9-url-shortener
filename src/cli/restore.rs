use crate::config::Config;
use flate2::read::GzDecoder;
use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;
use tar::Archive;
use tracing::{error, info};

pub fn perform_restore(
    file_path: &std::path::Path,
    data_dir: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Open the archive
    let f = File::open(file_path)?;
    let tar_gz = GzDecoder::new(f);
    let mut archive = Archive::new(tar_gz);

    // 2. Unpack to temporary directory first
    let temp_dir =
        std::env::temp_dir().join(format!("bzod_system_restore_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir)?;

    if let Err(e) = archive.unpack(&temp_dir) {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(e.into());
    }

    // 3. Run validation on temp_dir
    let mut temp_config = Config::load();
    temp_config.data_dir = temp_dir.clone();

    // Namespace audit
    match crate::db::users::audit_slug_namespace(&temp_config) {
        Ok(report) => {
            if !report.duplicates.is_empty() {
                let _ = std::fs::remove_dir_all(&temp_dir);
                return Err(
                    format!("Slug conflicts detected in backup: {:?}", report.duplicates).into(),
                );
            }
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err(format!("Failed to audit slug namespace in backup: {}", e).into());
        }
    }

    // Registry integrity check
    let system_db_path = if temp_dir.join("admin/system.db").exists() {
        temp_dir.join("admin/system.db")
    } else {
        temp_dir.join("system.db")
    };
    let users_db_path = if temp_dir.join("admin/users.db").exists() {
        temp_dir.join("admin/users.db")
    } else {
        temp_dir.join("users.db")
    };

    if system_db_path.exists() && users_db_path.exists() {
        let system_conn = rusqlite::Connection::open(&system_db_path)?;
        let users_conn = rusqlite::Connection::open(&users_db_path)?;
        match crate::db::users::verify_global_slug_registry_integrity(
            &system_conn,
            &users_conn,
            &temp_dir,
        ) {
            Ok((errors, _warnings)) => {
                if !errors.is_empty() {
                    let _ = std::fs::remove_dir_all(&temp_dir);
                    return Err(format!("Registry integrity errors in backup: {:?}", errors).into());
                }
            }
            Err(e) => {
                let _ = std::fs::remove_dir_all(&temp_dir);
                return Err(format!("Failed to verify registry integrity in backup: {}", e).into());
            }
        }
    }

    // 4. If validation succeeds, copy temp_dir contents to data_dir
    if data_dir.exists() {
        let _ = std::fs::remove_dir_all(data_dir);
    }
    std::fs::create_dir_all(data_dir)?;

    fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            if ty.is_dir() {
                copy_dir_all(&entry.path(), &dst.join(entry.file_name()))?;
            } else {
                std::fs::copy(entry.path(), dst.join(entry.file_name()))?;
            }
        }
        Ok(())
    }

    if let Err(e) = copy_dir_all(&temp_dir, data_dir) {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(format!("Failed to copy restored files: {}", e).into());
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(())
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

    info!(
        "WARNING: Restoring will overwrite existing databases in {:?}",
        config.data_dir
    );
    print!("Are you sure you want to restore? (y/N): ");
    let _ = io::stdout().flush();
    let mut confirm = String::new();
    let _ = io::stdin().read_line(&mut confirm);

    if !confirm.trim().eq_ignore_ascii_case("y") {
        info!("Restore cancelled.");
        return Ok(());
    }

    if !config.data_dir.exists() {
        std::fs::create_dir_all(&config.data_dir)?;
    }

    info!("Restoring backup from: {:?}", file_path);
    perform_restore(&file_path, &config.data_dir)?;
    info!("Database files successfully restored.");

    Ok(())
}
