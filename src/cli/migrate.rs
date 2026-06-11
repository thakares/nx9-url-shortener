use std::path::PathBuf;
use tracing::info;
use crate::config::Config;
use crate::db::Db;

pub async fn run(
    data_dir: Option<String>,
    dry_run: bool,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(d) = data_dir { config.data_dir = PathBuf::from(d); }

    if dry_run {
        info!("Dry run enabled: pending database migrations will be reported but not applied.");
        info!("Data directory: {:?}", config.data_dir);
        return Ok(());
    }

    info!("Running database migrations...");
    let _db = Db::init(&config)?;
    info!("Database migrations applied successfully.");

    Ok(())
}
