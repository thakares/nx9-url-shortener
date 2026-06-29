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

    let mut all_healthy = true;

    // Define target databases in the new layout
    let admin_dir = config.data_dir.join("admin");
    let legacy_user_dir = config.data_dir.join("users").join("1");

    let dbs = vec![
        ("admin", admin_dir.join("admin.db")),
        ("system", admin_dir.join("system.db")),
        ("users", admin_dir.join("users.db")),
        ("legacy content", legacy_user_dir.join("content.db")),
        ("legacy analytics", legacy_user_dir.join("analytics.db")),
    ];

    for (db_name, db_path) in dbs {
        // Skip legacy databases if they don't exist
        if db_name.starts_with("legacy") && !db_path.exists() {
            continue;
        }

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

    // Global Slug Registry Integrity Check
    println!("Global Slug Registry Integrity Check");
    println!("====================================");
    let system_db_path = admin_dir.join("system.db");
    let users_db_path = admin_dir.join("users.db");

    if system_db_path.exists() && users_db_path.exists() {
        match (
            Connection::open(&system_db_path),
            Connection::open(&users_db_path),
        ) {
            (Ok(sys_conn), Ok(usr_conn)) => {
                match crate::db::users::verify_global_slug_registry_integrity(
                    &sys_conn,
                    &usr_conn,
                    &config.data_dir,
                ) {
                    Ok((errors, warnings)) => {
                        if errors.is_empty() && warnings.is_empty() {
                            println!("  Status: HEALTHY (no issues found)");
                        } else {
                            if !errors.is_empty() {
                                println!("  Errors (Action Required):");
                                for err in &errors {
                                    println!("    - {}", err);
                                }
                                all_healthy = false;
                            }
                            if !warnings.is_empty() {
                                println!("  Warnings (Attention Needed):");
                                for warn in &warnings {
                                    println!("    - {}", warn);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        println!("  Status: ERROR running integrity check: {}", e);
                        all_healthy = false;
                    }
                }
            }
            _ => {
                println!("  Status: ERROR opening system.db or users.db for integrity check");
                all_healthy = false;
            }
        }
    } else {
        println!("  Status: SKIPPED (system.db/users.db not found)");
    }
    println!();

    println!("--------------------");
    if all_healthy {
        println!("Overall status: HEALTHY");
    } else {
        println!("Overall status: ISSUES DETECTED");
    }

    Ok(())
}
