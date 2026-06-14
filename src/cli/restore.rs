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

    // 2. Validate that the archive contains the expected BZOD database files
    let mut has_admin = false;
    let mut has_content = false;
    let mut has_analytics = false;
    let mut has_system = false;

    for entry_res in archive.entries()? {
        let entry = entry_res?;
        let path = entry.path()?;
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        match file_name {
            "admin.db" => has_admin = true,
            "content.db" => has_content = true,
            "analytics.db" => has_analytics = true,
            "system.db" => has_system = true,
            _ => {}
        }
    }

    if !has_admin || !has_content || !has_analytics || !has_system {
        return Err("Archive is missing one or more required database files (admin.db, content.db, analytics.db, system.db)".into());
    }

    // 3. Unpack archive to data_dir
    let f2 = File::open(file_path)?;
    let tar_gz2 = GzDecoder::new(f2);
    let mut archive2 = Archive::new(tar_gz2);
    archive2.unpack(data_dir)?;
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
