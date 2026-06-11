use std::path::PathBuf;
use std::fs::File;
use std::io::{self, Write};
use tracing::{info, error};
use flate2::read::GzDecoder;
use tar::Archive;
use crate::config::Config;

pub async fn run(
    file: String,
    data_dir: Option<String>,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(d) = data_dir { config.data_dir = PathBuf::from(d); }
    let file_path = PathBuf::from(file);

    if !file_path.exists() {
        error!("Backup file not found: {:?}", file_path);
        return Ok(());
    }

    info!("WARNING: Restoring will overwrite existing databases in {:?}", config.data_dir);
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
    let f = File::open(&file_path)?;
    let tar_gz = GzDecoder::new(f);
    let mut archive = Archive::new(tar_gz);
    archive.unpack(&config.data_dir)?;
    info!("Database files successfully restored.");

    Ok(())
}
