use crate::config::Config;
use crate::db::Db;
use rusqlite::Connection;
use std::path::PathBuf;
use tracing::{error, info};

pub async fn run(
    target_admin_id: i64,
    data_dir: Option<String>,
    dry_run: bool,
    force: bool,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(d) = data_dir {
        config.data_dir = PathBuf::from(d);
    }
    let db = Db::init(&config)?;

    // 1. Verify target admin exists and is an admin
    let target_user = {
        let conn = db.users.lock().unwrap();
        crate::db::users::get_user_by_id(&conn, target_admin_id)?
    };

    let target_user = match target_user {
        Some(u) => u,
        None => {
            error!("Target admin ID {} not found", target_admin_id);
            return Ok(());
        }
    };

    if target_user.account_type != "admin" {
        error!(
            "Target user '{}' (ID {}) is not an admin account.",
            target_user.username, target_admin_id
        );
        return Ok(());
    }

    if target_admin_id == 1 {
        error!("Target admin ID cannot be 1 (legacy admin).");
        return Ok(());
    }

    // 2. Open databases
    let legacy_content_path = config.data_dir.join("users").join("1").join("content.db");

    if !legacy_content_path.exists() {
        info!(
            "No legacy admin content database found at {:?}",
            legacy_content_path
        );
        return Ok(());
    }

    db.init_user_databases(target_admin_id)?;
    let target_content_path = config
        .data_dir
        .join("users")
        .join(target_admin_id.to_string())
        .join("content.db");

    let mut legacy_conn = Connection::open(&legacy_content_path)?;
    let mut target_conn = Connection::open(&target_content_path)?;
    let mut system_conn = db.system.lock().unwrap();

    println!("Scanning legacy admin content database...");

    // 3. Count items
    let urls = {
        let mut stmt = legacy_conn.prepare("SELECT * FROM urls;")?;
        let mut rows = stmt.query([])?;
        let mut data = Vec::new();
        while let Ok(Some(_)) = rows.next() {
            data.push(1);
        }
        data
    };
    let url_count = urls.len();

    let pages = {
        let mut stmt = legacy_conn.prepare("SELECT * FROM landing_pages;")?;
        let mut rows = stmt.query([])?;
        let mut data = Vec::new();
        while let Ok(Some(_)) = rows.next() {
            data.push(1);
        }
        data
    };
    let page_count = pages.len();

    println!(
        "Found {} URLs and {} Landing Pages owned by legacy admin (ID 1).",
        url_count, page_count
    );

    if dry_run {
        println!("Dry run mode enabled. No changes will be made.");
        return Ok(());
    }

    if !force {
        println!("Migration requires the --force flag to execute. Aborting.");
        return Ok(());
    }

    println!(
        "Starting migration to Admin '{}' (ID {})...",
        target_user.username, target_admin_id
    );

    // 4. Perform Migration (using ATTACH DATABASE for fast copy)
    // We attach the legacy db to the target db to do INSERT INTO ... SELECT * FROM
    target_conn.execute(
        "ATTACH DATABASE ?1 AS legacy;",
        rusqlite::params![legacy_content_path.to_string_lossy()],
    )?;

    let tx = target_conn.transaction()?;
    tx.execute("INSERT OR IGNORE INTO urls SELECT * FROM legacy.urls;", [])?;
    tx.execute(
        "INSERT OR IGNORE INTO landing_pages SELECT * FROM legacy.landing_pages;",
        [],
    )?;
    tx.commit()?;

    target_conn.execute("DETACH DATABASE legacy;", [])?;

    // 5. Update global registry
    let sys_tx = system_conn.transaction()?;
    let updated_slugs = sys_tx.execute(
        "UPDATE global_slugs SET owner_user_id = ?1 WHERE owner_user_id = 1;",
        rusqlite::params![target_admin_id],
    )?;
    sys_tx.commit()?;

    // 6. Delete from legacy
    let legacy_tx = legacy_conn.transaction()?;
    legacy_tx.execute("DELETE FROM urls;", [])?;
    legacy_tx.execute("DELETE FROM landing_pages;", [])?;
    legacy_tx.commit()?;

    println!("Migration Complete!");
    println!("-------------------");
    println!("Migrated {} URLs.", url_count);
    println!("Migrated {} Landing Pages.", page_count);
    println!("Updated {} slugs in global registry.", updated_slugs);
    println!("Cleared legacy content database.");

    Ok(())
}
