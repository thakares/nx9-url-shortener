use crate::config::Config;
use crate::db::sqlite;
use rusqlite::Connection;
use std::path::PathBuf;
use tracing::info;

/// Run comprehensive database diagnostics.
///
/// Opens each database, collects health reports (schema version, journal mode,
/// foreign key enforcement, integrity check), and prints a summary.
pub async fn run(
    data_dir: Option<String>,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(d) = data_dir {
        config.data_dir = PathBuf::from(d);
    }

    info!("Running BZOD database diagnostics...");
    println!("BZOD Database Doctor");
    println!("====================");
    println!("Data directory: {:?}", config.data_dir);
    println!();

    let databases = ["admin", "content", "analytics", "system"];
    let mut all_healthy = true;

    for db_name in &databases {
        let db_path = config.data_dir.join(format!("{}.db", db_name));

        if !db_path.exists() {
            println!("Database: {}", db_name);
            println!("  Status: NOT FOUND at {:?}", db_path);
            println!();
            all_healthy = false;
            continue;
        }

        match Connection::open(&db_path) {
            Ok(conn) => match sqlite::collect_health_report(&conn, db_name) {
                Ok(report) => {
                    println!("Database: {}", report.database);
                    println!("  Path:             {:?}", db_path);
                    println!("  Schema version:   {}", report.schema_version);
                    println!("  Journal mode:     {}", report.journal_mode);
                    println!(
                        "  Foreign keys:     {}",
                        if report.foreign_keys_enabled {
                            "enabled"
                        } else {
                            "DISABLED"
                        }
                    );
                    println!(
                        "  Integrity:        {}",
                        if report.integrity_ok { "ok" } else { "FAILED" }
                    );

                    if !report.integrity_ok || !report.foreign_keys_enabled {
                        all_healthy = false;
                    }
                }
                Err(e) => {
                    println!("Database: {}", db_name);
                    println!("  Status: ERROR collecting health report: {}", e);
                    all_healthy = false;
                }
            },
            Err(e) => {
                println!("Database: {}", db_name);
                println!("  Status: FAILED to open: {}", e);
                all_healthy = false;
            }
        }
        println!();
    }

    println!("--------------------");
    if all_healthy {
        println!("Overall status: HEALTHY");
    } else {
        println!("Overall status: ISSUES DETECTED");
    }

    Ok(())
}
