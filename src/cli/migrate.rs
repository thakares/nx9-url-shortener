use crate::config::Config;
use crate::db::Db;
use std::path::PathBuf;
use tracing::info;

pub async fn run(
    data_dir: Option<String>,
    dry_run: bool,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(d) = data_dir {
        config.data_dir = PathBuf::from(d);
    }

    if dry_run {
        info!("Dry run enabled: pending database migrations will be reported but not applied.");
        info!("Data directory: {:?}", config.data_dir);
        let id_report =
            crate::db::identity_migrate::run_identity_migration(&config, true, false).await?;
        println!("\nIdentity Migration Preflight Report (Dry Run):");
        println!("----------------------------------------------");
        println!("Total users: {}", id_report.total_users);
        println!(
            "Users needing TenantId: {}",
            id_report.users_assigned_tenant_id
        );
        println!("Users needing UUID: {}", id_report.users_assigned_uuid);
        println!("Directories to move: {}", id_report.directories_moved);
        println!(
            "Directories already migrated: {}",
            id_report.directories_already_migrated
        );
        println!(
            "Legacy admin (users/1) preserved: {}",
            id_report.legacy_admin_preserved
        );

        let slug_report =
            crate::db::slug_migrate::run_global_slug_migration(&config, true, false).await?;
        println!("\nGlobal Slug Migration Preflight Report (Dry Run):");
        println!("-------------------------------------------------");
        println!("Total legacy slugs: {}", slug_report.total_legacy_slugs);
        println!("URL slugs to migrate: {}", slug_report.url_slugs_migrated);
        println!("Page slugs to migrate: {}", slug_report.page_slugs_migrated);
        println!("Reserved slugs: {}", slug_report.reserved_slugs_migrated);
        return Ok(());
    }

    info!("Running database schema migrations...");
    let _db = Db::init(&config)?;
    info!("Database schema migrations applied successfully.");

    info!("Running identity & directory migration lifecycle...");
    let id_report =
        crate::db::identity_migrate::run_identity_migration(&config, false, false).await?;
    println!("\nIdentity Migration Report:");
    println!("--------------------------");
    println!("Total users: {}", id_report.total_users);
    println!("TenantIds assigned: {}", id_report.users_assigned_tenant_id);
    println!("UUIDs assigned: {}", id_report.users_assigned_uuid);
    println!("Directories migrated: {}", id_report.directories_moved);
    println!(
        "Directories already migrated: {}",
        id_report.directories_already_migrated
    );
    println!(
        "Legacy admin preserved: {}",
        id_report.legacy_admin_preserved
    );
    println!("Validation passed: {}", id_report.validation_passed);

    if !id_report.warnings.is_empty() {
        println!("\nWarnings:");
        for w in &id_report.warnings {
            println!("  - {}", w);
        }
    }

    info!("Running global slug migration lifecycle...");
    let slug_report =
        crate::db::slug_migrate::run_global_slug_migration(&config, false, false).await?;
    println!("\nGlobal Slug Migration Report:");
    println!("-----------------------------");
    println!("Total legacy slugs: {}", slug_report.total_legacy_slugs);
    println!("URL slugs migrated: {}", slug_report.url_slugs_migrated);
    println!("Page slugs migrated: {}", slug_report.page_slugs_migrated);
    println!(
        "Reserved slugs verified: {}",
        slug_report.reserved_slugs_migrated
    );
    println!(
        "Existing target records verified: {}",
        slug_report.existing_records_verified
    );
    println!("Validation passed: {}", slug_report.validation_passed);

    if !slug_report.warnings.is_empty() {
        println!("\nWarnings:");
        for w in &slug_report.warnings {
            println!("  - {}", w);
        }
    }

    Ok(())
}
