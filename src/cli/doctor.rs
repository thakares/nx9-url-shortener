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

    let topology = crate::db::topology::Topology::new(&config.data_dir);

    let dbs = vec![
        ("admin", topology.admin_db()),
        ("system", topology.system_db()),
        ("users", topology.users_registry_db()),
        ("global_urls", topology.global_urls_db()),
        ("global_landing_pages", topology.global_landing_pages_db()),
        ("reserved", topology.reserved_db()),
        (
            "legacy content",
            topology
                .content_db(crate::db::topology::LEGACY_ADMIN_USER_KEY)
                .expect("legacy admin user key is valid"),
        ),
        (
            "legacy analytics",
            topology
                .analytics_db(crate::db::topology::LEGACY_ADMIN_USER_KEY)
                .expect("legacy admin user key is valid"),
        ),
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
    let system_db_path = topology.system_db();
    let users_db_path = topology.users_registry_db();

    if system_db_path.exists() && users_db_path.exists() {
        match (
            Connection::open(&system_db_path),
            Connection::open(&users_db_path),
        ) {
            (Ok(sys_conn), Ok(usr_conn)) => {
                match crate::services::registry_validator::RegistryValidator::scan(
                    &sys_conn,
                    &usr_conn,
                    &config.data_dir,
                    None,
                ) {
                    Ok(issues) => {
                        if issues.is_empty() {
                            println!("  Status: HEALTHY (no issues found)");
                        } else {
                            println!("  Status: ISSUES DETECTED");
                            all_healthy = false;

                            for issue in &issues {
                                println!();
                                println!("ERROR");
                                println!();
                                println!("Slug:");
                                println!("    {}", issue.slug);
                                println!();
                                println!("Type:");
                                println!(
                                    "    {}",
                                    if issue.target_type == "url" {
                                        "URL"
                                    } else if issue.target_type == "page" {
                                        "Landing Page"
                                    } else {
                                        &issue.target_type
                                    }
                                );
                                println!();
                                println!("Owner:");
                                println!("    Tenant ID {}", issue.owner_tenant_id);
                                println!();
                                println!("Database:");
                                println!("    {}", issue.database_path.display());
                                println!();
                                println!("Target UUID:");
                                println!("    {}", issue.target_id);
                                println!();
                                println!("Issue:");
                                println!("    {:?}", issue.issue_type);
                                println!();
                                println!("Description:");
                                println!("    {}", issue.description);
                                println!();
                                println!("Suggested Repair:");
                                println!();
                                if issue.slug != "*" {
                                    println!(
                                        "    bzod repair registry --slug {} --dry-run",
                                        issue.slug
                                    );
                                } else {
                                    println!("    bzod repair registry --dry-run");
                                }
                                println!("--------------------");
                            }
                        }
                    }
                    Err(e) => {
                        println!("  Status: ERROR running registry scan: {}", e);
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
