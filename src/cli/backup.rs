use crate::config::Config;
use crate::db::Db;
use crate::jobs::backup::perform_backup;
use std::path::PathBuf;
use tracing::info;

pub async fn run(
    out: Option<String>,
    data_dir: Option<String>,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(d) = data_dir {
        config.data_dir = PathBuf::from(d);
    }
    if let Some(o) = out {
        config.backup_dir = PathBuf::from(o);
    }

    // Init DB connections to ensure databases exist and migrate if needed
    let db = Db::init(&config)?;

    info!("Starting database backup...");
    let backup_path = perform_backup(&db, &config).await?;
    info!("Database backup generated successfully: {}", backup_path);

    Ok(())
}
