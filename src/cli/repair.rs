use crate::cli::RepairCommands;
use crate::config::Config;
use crate::services::registry_validator::{RegistryIssueType, RegistryValidator};
use rusqlite::Connection;
use std::path::PathBuf;
use tracing::info;

pub async fn run(
    command: RepairCommands,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        RepairCommands::Registry {
            dry_run,
            force,
            slug,
            data_dir,
        } => {
            if let Some(d) = data_dir {
                config.data_dir = PathBuf::from(d);
            }

            if !dry_run && !force {
                println!("Error: You must specify either --dry-run or --force");
                return Ok(());
            }
            if dry_run && force {
                println!("Error: Cannot specify both --dry-run and --force");
                return Ok(());
            }

            let start_time = std::time::Instant::now();
            let topology = crate::db::topology::Topology::new(&config.data_dir);
            let system_db_path = topology.system_db();
            let users_db_path = topology.users_registry_db();
            let urls_db_path = topology.global_urls_db();
            let pages_db_path = topology.global_landing_pages_db();

            if !system_db_path.exists()
                || !users_db_path.exists()
                || !urls_db_path.exists()
                || !pages_db_path.exists()
            {
                println!("Error: Core databases (system.db, users.db, slugs/*.db) not found.");
                return Ok(());
            }

            let sys_conn = Connection::open(&system_db_path)?;
            let usr_conn = Connection::open(&users_db_path)?;
            let mut urls_conn = Connection::open(&urls_db_path)?;
            let mut pages_conn = Connection::open(&pages_db_path)?;

            let slug_filter = slug.as_deref();

            if dry_run {
                println!("BZOD Registry Repair (v0.8)\n");
                println!("Scanning Authoritative Slug Registries (global_urls.db, global_landing_pages.db)...");

                let issues =
                    RegistryValidator::scan(&sys_conn, &usr_conn, &config.data_dir, slug_filter)?;

                let true_orphans = issues
                    .iter()
                    .filter(|i| {
                        matches!(
                            i.issue_type,
                            RegistryIssueType::MissingTarget
                                | RegistryIssueType::TrueOrphan
                                | RegistryIssueType::MissingTenant
                        )
                    })
                    .collect::<Vec<_>>();

                let corrupt = issues
                    .iter()
                    .filter(|i| i.issue_type == RegistryIssueType::CorruptDatabase)
                    .collect::<Vec<_>>();

                let access_failures = issues
                    .iter()
                    .filter(|i| i.issue_type == RegistryIssueType::AccessFailure)
                    .collect::<Vec<_>>();

                println!("\nDetected Issues (Total: {}):", issues.len());
                println!("  Orphaned/Missing Targets: {}", true_orphans.len());
                println!("  Corrupt Databases (Protected): {}", corrupt.len());
                println!("  Access Failures (Protected): {}", access_failures.len());

                if !true_orphans.is_empty() {
                    println!("\nThe following orphaned entries would be repaired/removed:");
                    for issue in &true_orphans {
                        println!(
                            "  {} [{}] — Tenant: {}",
                            issue.slug,
                            issue.target_type.to_uppercase(),
                            issue.owner_tenant_id
                        );
                    }
                }

                if !corrupt.is_empty() {
                    println!("\nProtected from destructive repair (Corrupt DBs):");
                    for issue in &corrupt {
                        println!(
                            "  {} [{}] — Path: {:?}",
                            issue.slug,
                            issue.target_type.to_uppercase(),
                            issue.database_path
                        );
                    }
                }

                println!("\nNo changes have been made (dry-run).");
                println!(
                    "Run again with:\n    bzod repair registry --force{}",
                    if let Some(s) = slug_filter {
                        format!(" --slug {}", s)
                    } else {
                        "".to_string()
                    }
                );

                info!(
                    "Registry Repair Scan completed. Orphaned: {}, Corrupt: {}, Duration: {:?}",
                    true_orphans.len(),
                    corrupt.len(),
                    start_time.elapsed()
                );
            } else if force {
                let issues =
                    RegistryValidator::scan(&sys_conn, &usr_conn, &config.data_dir, slug_filter)?;

                // Only repair true orphans, missing targets, or missing tenants.
                // Never delete records with CorruptDatabase or AccessFailure per safety policies.
                let repairable = issues
                    .into_iter()
                    .filter(|i| {
                        matches!(
                            i.issue_type,
                            RegistryIssueType::MissingTarget
                                | RegistryIssueType::TrueOrphan
                                | RegistryIssueType::MissingTenant
                        )
                    })
                    .collect::<Vec<_>>();

                if repairable.is_empty() {
                    println!("No repairable registry issues found.");
                    return Ok(());
                }

                let tx_urls = urls_conn.transaction()?;
                let tx_pages = pages_conn.transaction()?;

                let mut removed_count = 0;
                for issue in &repairable {
                    if issue.target_type == "url" {
                        removed_count += tx_urls
                            .execute("DELETE FROM global_urls WHERE slug = ?1;", [&issue.slug])?;
                    } else {
                        removed_count += tx_pages.execute(
                            "DELETE FROM global_landing_pages WHERE slug = ?1;",
                            [&issue.slug],
                        )?;
                    }
                }

                tx_urls.commit()?;
                tx_pages.commit()?;

                // Record audit event in system.db
                let _ = crate::db::audit_events::write_audit_event(
                    &sys_conn,
                    "cli",
                    "REGISTRY_REPAIR",
                    "registry",
                    slug_filter.unwrap_or("*"),
                    Some(&format!(
                        "Repaired/removed {} orphaned slug entries",
                        removed_count
                    )),
                );

                println!("Registry Repair Complete.");
                println!("Repaired/Removed: {} entries", removed_count);

                info!(
                    "Registry Repair Complete. Removed: {}. Duration: {:?}",
                    removed_count,
                    start_time.elapsed()
                );
            }
        }
    }
    Ok(())
}
