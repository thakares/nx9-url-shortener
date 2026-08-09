use crate::config::Config;
use crate::services::registry_validator::RegistryIssueType;
use flate2::read::GzDecoder;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tar::Archive;
use tracing::{error, info, warn};

/// Read backup_manifest.json and return true if this is a legacy_flat_backup.
fn is_legacy_flat_backup(temp_dir: &Path) -> bool {
    let manifest_path = temp_dir.join("backup_manifest.json");
    if !manifest_path.exists() {
        return false;
    }
    match std::fs::read_to_string(&manifest_path) {
        Ok(contents) => match serde_json::from_str::<serde_json::Value>(&contents) {
            Ok(val) => val.get("type").and_then(|t| t.as_str()) == Some("legacy_flat_backup"),
            Err(_) => false,
        },
        Err(_) => false,
    }
}

/// Detect if the unpacked archive is in flat layout (files at root, not in admin/ subdirectory).
fn is_flat_layout(temp_dir: &Path) -> bool {
    temp_dir.join("admin.db").exists() && !temp_dir.join("admin").join("admin.db").exists()
}

/// Bootstrap users.db for a legacy backup where users.db is empty/unmigrated.
///
/// This function:
/// 1. Runs USERS_MIGRATIONS on users.db to create the required schema.
/// 2. Reads the actual administrator identity from admin.db (preserving
///    the original username and argon2id password hash — no manufacturing).
/// 3. Creates a legacy_admin system placeholder (id=1) for tenant ownership.
/// 4. Creates an admin account with the original credentials.
/// 5. Scans global_slugs for owner_user_ids and creates disabled placeholder
///    accounts for any missing tenants.
fn bootstrap_legacy_users_db(temp_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use crate::db::migrations::{run_migrations, USERS_MIGRATIONS};

    let users_db_path = temp_dir.join("admin").join("users.db");
    let admin_db_path = temp_dir.join("admin").join("admin.db");
    let system_db_path = temp_dir.join("admin").join("system.db");

    // Check if users.db already has the users table (i.e., not a legacy backup)
    {
        let conn = rusqlite::Connection::open(&users_db_path)?;
        let has_users_table: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='users');",
                [],
                |r| r.get(0),
            )
            .unwrap_or(false);
        if has_users_table {
            info!("users.db already has users table; skipping legacy bootstrap");
            return Ok(());
        }
    }

    info!("Legacy users.db detected (empty/unmigrated). Bootstrapping current schema...");

    // Step 1: Run migrations to create the users.db schema
    let mut users_conn = rusqlite::Connection::open(&users_db_path)?;
    crate::db::sqlite::enable_wal(&users_conn, "users")?;
    crate::db::sqlite::enable_foreign_keys(&users_conn, "users")?;
    run_migrations(&mut users_conn, "users", USERS_MIGRATIONS, None)?;

    // Step 2: Read the actual administrator identity from admin.db
    let (admin_username, admin_password_hash) = {
        let admin_conn = rusqlite::Connection::open(&admin_db_path)?;

        // The legacy admin.db users table has schema:
        //   id TEXT PRIMARY KEY (UUID), username TEXT, password_hash TEXT, created_at TEXT
        // Read the actual admin — typically the first (and often only) user.
        let result: Result<(String, String), _> = admin_conn.query_row(
            "SELECT username, password_hash FROM users ORDER BY created_at ASC LIMIT 1;",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );

        match result {
            Ok((username, hash)) => {
                info!(
                    "Preserved administrator identity from legacy admin.db: username='{}'",
                    username
                );
                (username, hash)
            }
            Err(e) => {
                return Err(format!(
                    "Failed to read administrator credentials from legacy admin.db: {}",
                    e
                )
                .into());
            }
        }
    };

    // Step 3: Create legacy_admin system placeholder (id=1) for tenant content ownership
    // This account owns the content.db/analytics.db from the flat backup (users/1/).
    // It uses the original admin's password hash so no synthetic credentials are introduced.
    let now = chrono::Utc::now().to_rfc3339();
    users_conn.execute(
        "INSERT INTO users (id, username, password_hash, status, created_at, account_type)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6);",
        rusqlite::params![
            1i64,
            "legacy_admin",
            &admin_password_hash,
            "disabled",
            &now,
            "system"
        ],
    )?;
    users_conn.execute("INSERT INTO quotas (user_id) VALUES (?1);", [1i64])?;
    info!("Created legacy_admin system account (id=1) for tenant content ownership");

    // Step 4: Create the actual admin account with original credentials
    users_conn.execute(
        "INSERT INTO users (username, password_hash, status, created_at, account_type)
         VALUES (?1, ?2, ?3, ?4, ?5);",
        rusqlite::params![
            &admin_username,
            &admin_password_hash,
            "active",
            &now,
            "admin"
        ],
    )?;
    let admin_id = users_conn.last_insert_rowid();
    users_conn.execute("INSERT INTO quotas (user_id) VALUES (?1);", [admin_id])?;
    info!(
        "Created admin account '{}' (id={}) with original credentials",
        admin_username, admin_id
    );

    // Step 5: Scan global_slugs for owner_user_ids and create placeholders for missing tenants
    if system_db_path.exists() {
        let system_conn = rusqlite::Connection::open(&system_db_path)?;
        let has_global_slugs: bool = system_conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='global_slugs');",
                [],
                |r| r.get(0),
            )
            .unwrap_or(false);

        if has_global_slugs {
            let mut stmt =
                system_conn.prepare("SELECT DISTINCT owner_user_id FROM global_slugs;")?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let owner_id: i64 = row.get(0)?;
                // Skip user 1 (legacy_admin) and the admin we just created
                if owner_id == 1 || owner_id == admin_id {
                    continue;
                }

                // Check if this user already exists in users.db
                let exists: bool = users_conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM users WHERE id = ?1);",
                        [owner_id],
                        |r| r.get(0),
                    )
                    .unwrap_or(false);

                if !exists {
                    // Create a disabled placeholder so RegistryValidator can resolve ownership.
                    // The tenant's actual databases were not included in the flat backup.
                    let placeholder_name = format!("restored_user_{}", owner_id);
                    users_conn.execute(
                        "INSERT INTO users (id, username, password_hash, status, created_at, account_type, metadata)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);",
                        rusqlite::params![
                            owner_id,
                            &placeholder_name,
                            &admin_password_hash,
                            "disabled",
                            &now,
                            "standard",
                            "Placeholder created during legacy_flat_backup restore. Original tenant databases were not included in the flat backup."
                        ],
                    )?;
                    users_conn.execute("INSERT INTO quotas (user_id) VALUES (?1);", [owner_id])?;
                    warn!(
                        "Created placeholder account for user_id={} (referenced in global_slugs but tenant databases not in backup)",
                        owner_id
                    );
                }
            }
        }
    }

    Ok(())
}

/// Classify registry issues into hard errors vs warnings for legacy restore.
///
/// Hard errors: DuplicateSlug, InvalidTargetType, InvalidStatus
/// Warnings:    MissingDatabase, MissingTarget, MissingOwner, StaleReservation,
///              TenantAdminHasIsolatedContent
fn classify_registry_issues(
    issues: &[crate::services::registry_validator::RegistryIssue],
    is_legacy: bool,
) -> (
    Vec<&crate::services::registry_validator::RegistryIssue>,
    Vec<&crate::services::registry_validator::RegistryIssue>,
) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    for issue in issues {
        match issue.issue_type {
            RegistryIssueType::DuplicateSlug
            | RegistryIssueType::InvalidTargetType
            | RegistryIssueType::InvalidStatus => {
                errors.push(issue);
            }
            RegistryIssueType::MissingOwner if !is_legacy => {
                errors.push(issue);
            }
            _ => {
                // For legacy restores: MissingDatabase, MissingTarget, MissingOwner,
                // StaleReservation, TenantAdminHasIsolatedContent are warnings.
                // These represent pre-existing inconsistencies in the backup data,
                // not restore corruption.
                warnings.push(issue);
            }
        }
    }

    (errors, warnings)
}

pub fn perform_restore(
    file_path: &Path,
    data_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Open and unpack the archive to a temporary directory
    let f = File::open(file_path)?;
    let tar_gz = GzDecoder::new(f);
    let mut archive = Archive::new(tar_gz);

    let temp_dir =
        std::env::temp_dir().join(format!("bzod_system_restore_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir)?;

    if let Err(e) = archive.unpack(&temp_dir) {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(e.into());
    }

    // 2. Detect backup format
    let is_legacy = is_legacy_flat_backup(&temp_dir);
    let needs_normalization = is_flat_layout(&temp_dir);

    if is_legacy {
        info!("Detected legacy_flat_backup format — using legacy-aware restore path");
    }

    // 3. Normalize flat layout into multi-tenant structure BEFORE any validation
    if needs_normalization {
        info!("Normalizing flat database layout into multi-tenant structure...");
        if let Err(e) = crate::services::backup_layout::normalize_restored_layout(&temp_dir) {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err(format!("Failed to normalize legacy layout: {}", e).into());
        }
    }

    // 4. For legacy backups: bootstrap the empty users.db with the current schema
    //    and populate it from admin.db credentials
    if is_legacy {
        if let Err(e) = bootstrap_legacy_users_db(&temp_dir) {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err(format!("Failed to bootstrap legacy users database: {}", e).into());
        }
    }

    // 5. Run validation on the normalized temp_dir
    let mut temp_config = Config::load();
    temp_config.data_dir = temp_dir.clone();

    // Namespace audit
    match crate::db::users::audit_slug_namespace(&temp_config) {
        Ok(report) => {
            if !report.duplicates.is_empty() {
                let _ = std::fs::remove_dir_all(&temp_dir);
                return Err(
                    format!("Slug conflicts detected in backup: {:?}", report.duplicates).into(),
                );
            }
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err(format!("Failed to audit slug namespace in backup: {}", e).into());
        }
    }

    // Registry integrity check
    let system_db_path = temp_dir.join("admin").join("system.db");
    let users_db_path = temp_dir.join("admin").join("users.db");

    if system_db_path.exists() && users_db_path.exists() {
        let system_conn = rusqlite::Connection::open(&system_db_path)?;
        let users_conn = rusqlite::Connection::open(&users_db_path)?;
        match crate::services::registry_validator::RegistryValidator::scan(
            &system_conn,
            &users_conn,
            &temp_dir,
            None,
        ) {
            Ok(issues) => {
                if !issues.is_empty() {
                    let (hard_errors, warnings) = classify_registry_issues(&issues, is_legacy);

                    // Log all warnings
                    for w in &warnings {
                        warn!(
                            "Legacy restore warning: {:?} — {}",
                            w.issue_type, w.description
                        );
                    }

                    // Abort only on hard errors
                    if !hard_errors.is_empty() {
                        let descriptions: Vec<String> = hard_errors
                            .iter()
                            .map(|e| format!("{:?}: {}", e.issue_type, e.description))
                            .collect();
                        let _ = std::fs::remove_dir_all(&temp_dir);
                        return Err(format!(
                            "Registry integrity errors in backup ({} critical): {}",
                            hard_errors.len(),
                            descriptions.join("; ")
                        )
                        .into());
                    }

                    if !warnings.is_empty() {
                        info!(
                            "Registry validation completed with {} warnings (pre-existing backup inconsistencies)",
                            warnings.len()
                        );
                    }
                }
            }
            Err(e) => {
                let _ = std::fs::remove_dir_all(&temp_dir);
                return Err(format!("Failed to verify registry integrity in backup: {}", e).into());
            }
        }
    }

    // 6. If validation succeeds, atomically replace data_dir contents
    if data_dir.exists() {
        let _ = std::fs::remove_dir_all(data_dir);
    }
    std::fs::create_dir_all(data_dir)?;

    fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            if ty.is_dir() {
                copy_dir_all(&entry.path(), &dst.join(entry.file_name()))?;
            } else {
                std::fs::copy(entry.path(), dst.join(entry.file_name()))?;
            }
        }
        Ok(())
    }

    if let Err(e) = copy_dir_all(&temp_dir, data_dir) {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(format!("Failed to copy restored files: {}", e).into());
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(())
}

pub async fn run(
    file: String,
    data_dir: Option<String>,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(d) = data_dir {
        config.data_dir = PathBuf::from(d);
    }
    let file_path = PathBuf::from(file);

    if !file_path.exists() {
        error!("Backup file not found: {:?}", file_path);
        return Ok(());
    }

    info!(
        "WARNING: Restoring will overwrite existing databases in {:?}",
        config.data_dir
    );
    print!("Are you sure you want to restore? (y/N): ");
    let _ = io::stdout().flush();
    let mut confirm = String::new();
    let _ = io::stdin().read_line(&mut confirm);

    if !confirm.trim().eq_ignore_ascii_case("y") {
        info!("Restore cancelled.");
        return Ok(());
    }

    if !config.data_dir.exists() {
        std::fs::create_dir_all(&config.data_dir)?;
    }

    info!("Restoring backup from: {:?}", file_path);
    perform_restore(&file_path, &config.data_dir)?;
    info!("Database files successfully restored.");

    Ok(())
}
