use crate::config::Config;
use crate::db::Db;
use crate::services::destination_audit::{audit_all_destinations, format_report};
use std::path::PathBuf;
use tracing::info;

/// Read-only audit of all stored redirect destinations.
///
/// Does not rewrite, delete, or "repair" any records.
pub async fn run(
    data_dir: Option<String>,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(d) = data_dir {
        config.data_dir = PathBuf::from(d);
    }

    info!("Starting read-only destination audit...");
    let db = Db::init(&config)?;
    let report = audit_all_destinations(&db)?;
    print!("{}", format_report(&report));

    if report.invalid > 0 {
        // Non-zero exit so automation can detect findings without treating them as crashes.
        Err(format!(
            "destination audit found {} invalid stored URL(s)",
            report.invalid
        )
        .into())
    } else {
        Ok(())
    }
}
