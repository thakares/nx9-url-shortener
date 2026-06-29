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
            let admin_dir = config.data_dir.join("admin");
            let system_db_path = admin_dir.join("system.db");
            let users_db_path = admin_dir.join("users.db");

            if !system_db_path.exists() || !users_db_path.exists() {
                println!("Error: system.db or users.db not found.");
                return Ok(());
            }

            let mut sys_conn = Connection::open(&system_db_path)?;
            let usr_conn = Connection::open(&users_db_path)?;

            let slug_filter = slug.as_deref();

            if dry_run {
                println!("BZOD Registry Repair\n");
                println!("Scanning Global Slug Registry...");

                let issues =
                    RegistryValidator::scan(&sys_conn, &usr_conn, &config.data_dir, slug_filter)?;
                let orphaned = issues
                    .into_iter()
                    .filter(|i| {
                        matches!(
                            i.issue_type,
                            RegistryIssueType::MissingTarget
                                | RegistryIssueType::MissingDatabase
                                | RegistryIssueType::MissingOwner
                        )
                    })
                    .collect::<Vec<_>>();

                let orphaned_pages = orphaned.iter().filter(|i| i.target_type == "page").count();
                let orphaned_urls = orphaned.iter().filter(|i| i.target_type == "url").count();

                println!("\nDetected:");
                println!("\nPages:\n    {} orphaned", orphaned_pages);
                println!("\nURLs:\n    {} orphaned", orphaned_urls);

                if !orphaned.is_empty() {
                    println!("\nThe following entries would be removed:");
                    for issue in &orphaned {
                        println!("\n{}\n    {}", issue.target_type.to_uppercase(), issue.slug);
                    }
                }

                println!("\nNo changes have been made.");
                println!(
                    "\nRun again with:\n\n    bzod repair registry --force{}",
                    if let Some(s) = slug_filter {
                        format!(" --slug {}", s)
                    } else {
                        "".to_string()
                    }
                );

                info!(
                    "Registry Repair Started. Scanned. Orphaned Pages: {}, Orphaned URLs: {}. Duration: {:?}",
                    orphaned_pages, orphaned_urls, start_time.elapsed()
                );
            } else if force {
                let tx = sys_conn.transaction()?;

                let issues =
                    RegistryValidator::scan(&tx, &usr_conn, &config.data_dir, slug_filter)?;
                let orphaned = issues
                    .into_iter()
                    .filter(|i| {
                        matches!(
                            i.issue_type,
                            RegistryIssueType::MissingTarget
                                | RegistryIssueType::MissingDatabase
                                | RegistryIssueType::MissingOwner
                        )
                    })
                    .collect::<Vec<_>>();

                let orphaned_pages = orphaned.iter().filter(|i| i.target_type == "page").count();
                let orphaned_urls = orphaned.iter().filter(|i| i.target_type == "url").count();

                if orphaned.is_empty() {
                    println!("No repairs required.");
                    return Ok(());
                }

                if let Some(s) = slug_filter {
                    println!("Checking slug:\n\n{}\n", s);
                    if let Some(issue) = orphaned.first() {
                        println!("Owner:\n\n{}\n", issue.owner_user_id);
                        println!("Status:\n\nOrphaned\n");
                    }
                }

                let mut removed_count = 0;
                for issue in &orphaned {
                    let rows = tx.execute(
                        "DELETE FROM global_slugs WHERE slug = ?1",
                        rusqlite::params![issue.slug],
                    )?;
                    removed_count += rows;
                }

                tx.commit()?;

                if slug_filter.is_some() {
                    println!("Removed:\n\nSUCCESS");
                } else {
                    println!("Repair Complete\n");
                    println!("Removed:\n");
                    println!("Pages:\n    {}\n", orphaned_pages);
                    println!("URLs:\n    {}\n", orphaned_urls);

                    let remaining: i64 =
                        sys_conn
                            .query_row("SELECT COUNT(*) FROM global_slugs;", [], |r| r.get(0))?;
                    println!("Remaining Registry Entries:\n    {}\n", remaining);

                    let post_issues =
                        RegistryValidator::scan(&sys_conn, &usr_conn, &config.data_dir, None)?;
                    let post_orphaned = post_issues
                        .iter()
                        .filter(|i| {
                            matches!(
                                i.issue_type,
                                RegistryIssueType::MissingTarget
                                    | RegistryIssueType::MissingDatabase
                                    | RegistryIssueType::MissingOwner
                            )
                        })
                        .count();

                    println!(
                        "Integrity:\n    {}",
                        if post_orphaned == 0 { "PASS" } else { "FAIL" }
                    );
                }

                info!(
                    "Registry Repair Started. Scanned. Orphaned Pages: {}, Orphaned URLs: {}. Removed: {}. Duration: {:?}",
                    orphaned_pages, orphaned_urls, removed_count, start_time.elapsed()
                );
            }
        }
    }
    Ok(())
}
