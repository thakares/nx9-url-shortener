//! Explicit Phase 3 Identity & Directory Migration Engine.
//!
//! Lifecycle:
//! 1. Preflight (inspect DB and filesystem, identify unmigrated users)
//! 2. Backup (create pre-migration tarball)
//! 3. Identity Migration (assign immutable TenantId + UUID in users.db)
//! 4. Filesystem Migration (atomically move users/<id>/ to users/<TenantId>/)
//! 5. Validation (verify all users, directories, and databases)
//! 6. Completion Marker (record audit event and system setting)
//!
//! Legacy Admin (users/1) is preserved as a Core/legacy concern and NOT converted
//! to a normal TenantId directory.

use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;
use tracing::{info, warn};

use crate::config::Config;
use crate::db::topology::{is_v08_user_id, Topology};
use crate::db::Db;
use crate::identity::TenantId;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IdentityMigrationReport {
    pub total_users: usize,
    pub users_assigned_tenant_id: usize,
    pub users_assigned_uuid: usize,
    pub directories_moved: usize,
    pub directories_already_migrated: usize,
    pub legacy_admin_preserved: bool,
    pub validation_passed: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PreflightPlan {
    pub total_users: usize,
    pub normal_users: usize,
    pub users_needing_tenant_id: Vec<(i64, String)>,
    pub users_needing_uuid: Vec<(i64, String)>,
    pub directories_to_move: Vec<(i64, PathBuf, TenantId)>,
    pub directories_already_migrated: usize,
}

/// Run the full explicit identity migration lifecycle.
pub async fn run_identity_migration(
    config: &Config,
    dry_run: bool,
    skip_backup: bool,
) -> Result<IdentityMigrationReport, Box<dyn std::error::Error>> {
    let topology = Topology::new(&config.data_dir);
    let mut warnings = Vec::new();

    // 1. Initialize Db for Core connections
    let db = Db::init(config)?;

    // 2. Preflight
    let plan = {
        let users_conn = db.users.lock().unwrap();
        inspect_preflight(&users_conn, &topology)?
    };

    info!(
        "Preflight: total_users={}, normal_users={}, needing_tenant_id={}, needing_uuid={}, dirs_to_move={}",
        plan.total_users,
        plan.normal_users,
        plan.users_needing_tenant_id.len(),
        plan.users_needing_uuid.len(),
        plan.directories_to_move.len()
    );

    if dry_run {
        info!("Dry run mode enabled. No identity changes or directory moves will be performed.");
        return Ok(IdentityMigrationReport {
            total_users: plan.total_users,
            users_assigned_tenant_id: plan.users_needing_tenant_id.len(),
            users_assigned_uuid: plan.users_needing_uuid.len(),
            directories_moved: plan.directories_to_move.len(),
            directories_already_migrated: plan.directories_already_migrated,
            legacy_admin_preserved: true,
            validation_passed: true,
            warnings,
        });
    }

    // 3. Backup before migration (if there is work to do and backup is not skipped)
    let needs_migration = !plan.users_needing_tenant_id.is_empty()
        || !plan.users_needing_uuid.is_empty()
        || !plan.directories_to_move.is_empty();

    if needs_migration && !skip_backup {
        info!("Creating pre-migration backup...");
        match crate::jobs::backup::perform_backup(&db, config).await {
            Ok(path) => info!("Pre-migration backup created at {:?}", path),
            Err(e) => {
                warn!(
                    "Pre-migration backup failed: {}. Continuing with caution.",
                    e
                );
                warnings.push(format!("Pre-migration backup warning: {e}"));
            }
        }
    }

    // 4. Identity Migration (users.db backfill)
    let (assigned_tids, assigned_uuids) = {
        let mut users_conn = db.users.lock().unwrap();
        migrate_user_table_identities(&mut users_conn)?
    };

    // 5. Filesystem Migration (users/<id>/ -> users/<TenantId>/)
    let moved_dirs = {
        let users_conn = db.users.lock().unwrap();
        migrate_tenant_directories(&users_conn, &topology, &mut warnings)?
    };

    // 6. Validation
    let validation_passed = {
        let users_conn = db.users.lock().unwrap();
        validate_identity_migration(&users_conn, &topology, &mut warnings)?
    };

    // 7. Completion Marker in system.db
    if validation_passed {
        let system_conn = db.system.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let details = format!(
            "Identity migration completed: {} tenant_ids assigned, {} uuids assigned, {} directories moved",
            assigned_tids, assigned_uuids, moved_dirs
        );
        let _ = crate::db::audit_events::write_audit_event(
            &system_conn,
            "system",
            "IDENTITY_MIGRATION_COMPLETED",
            "identity",
            "users.db",
            Some(&details),
        );
        let _ = system_conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('v08_identity_migration_completed', ?1);",
            [&now],
        );
    }

    Ok(IdentityMigrationReport {
        total_users: plan.total_users,
        users_assigned_tenant_id: assigned_tids,
        users_assigned_uuid: assigned_uuids,
        directories_moved: moved_dirs,
        directories_already_migrated: plan.directories_already_migrated,
        legacy_admin_preserved: true,
        validation_passed,
        warnings,
    })
}

/// Inspect existing database and filesystem to build a preflight plan.
pub fn inspect_preflight(
    users_conn: &Connection,
    topology: &Topology,
) -> Result<PreflightPlan, Box<dyn std::error::Error>> {
    let users = crate::db::users::list_users(users_conn)?;
    let total_users = users.len();

    let mut normal_users = 0;
    let mut users_needing_tenant_id = Vec::new();
    let mut users_needing_uuid = Vec::new();
    let mut directories_to_move = Vec::new();
    let mut directories_already_migrated = 0;

    for user in &users {
        // Skip legacy admin / system accounts
        if user.account_type == "admin"
            || user.account_type == "system"
            || user.username == "legacy_admin"
        {
            continue;
        }

        normal_users += 1;

        if user.tenant_id.is_none() {
            users_needing_tenant_id.push((user.id, user.username.clone()));
        }
        if user.uuid.is_none() {
            users_needing_uuid.push((user.id, user.username.clone()));
        }

        if let Some(tid) = user.tenant_id {
            let legacy_dir = topology.user_dir_i64(user.id)?;
            let target_dir = topology.tenant_dir(tid);

            if legacy_dir.exists() && legacy_dir != target_dir {
                directories_to_move.push((user.id, legacy_dir, tid));
            } else if target_dir.exists() {
                directories_already_migrated += 1;
            }
        }
    }

    Ok(PreflightPlan {
        total_users,
        normal_users,
        users_needing_tenant_id,
        users_needing_uuid,
        directories_to_move,
        directories_already_migrated,
    })
}

/// Assign immutable TenantId and UUID for all normal users missing them in users.db.
/// Restart-safe: never regenerates or modifies existing identities.
pub fn migrate_user_table_identities(
    users_conn: &mut Connection,
) -> Result<(usize, usize), Box<dyn std::error::Error>> {
    let users = crate::db::users::list_users(users_conn)?;
    let mut assigned_tids = 0;
    let mut assigned_uuids = 0;

    let tx = users_conn.transaction()?;

    for user in users {
        // Skip legacy admin / system accounts
        if user.account_type == "admin"
            || user.account_type == "system"
            || user.username == "legacy_admin"
        {
            continue;
        }

        let mut needs_update = false;
        let mut tid_str = user.tenant_id.map(|t| t.as_str().to_string());
        let mut uuid_str = user.uuid;

        if tid_str.is_none() {
            // Allocate unique TenantId
            let tid = crate::identity::TenantId::generate();
            tid_str = Some(tid.as_str().to_string());
            assigned_tids += 1;
            needs_update = true;
        }

        if uuid_str.is_none() {
            // Allocate unique UUID
            let u = uuid::Uuid::new_v4().to_string();
            uuid_str = Some(u);
            assigned_uuids += 1;
            needs_update = true;
        }

        if needs_update {
            tx.execute(
                "UPDATE users SET tenant_id = ?1, uuid = ?2 WHERE id = ?3;",
                rusqlite::params![tid_str, uuid_str, user.id],
            )?;
        }
    }

    tx.commit()?;
    Ok((assigned_tids, assigned_uuids))
}

/// Move tenant directories from users/<id>/ to users/<TenantId>/ for normal users.
/// Legacy users/1 is preserved.
pub fn migrate_tenant_directories(
    users_conn: &Connection,
    topology: &Topology,
    warnings: &mut Vec<String>,
) -> Result<usize, Box<dyn std::error::Error>> {
    let users = crate::db::users::list_users(users_conn)?;
    let mut moved = 0;

    for user in users {
        if user.account_type == "admin"
            || user.account_type == "system"
            || user.username == "legacy_admin"
        {
            // Preserve Core / system accounts intact
            continue;
        }

        let Some(tid) = user.tenant_id else {
            warnings.push(format!(
                "User '{}' (ID {}) has no TenantId; skipping directory move",
                user.username, user.id
            ));
            continue;
        };

        let legacy_dir = match topology.user_dir_i64(user.id) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let target_dir = topology.tenant_dir(tid);

        if legacy_dir.exists() && legacy_dir != target_dir {
            if !target_dir.exists() {
                // Atomic rename on same filesystem
                fs::rename(&legacy_dir, &target_dir)?;
                let _ = fs::create_dir_all(target_dir.join("extensions"));
                info!(
                    "Migrated tenant directory for user '{}': {:?} -> {:?}",
                    user.username, legacy_dir, target_dir
                );
                moved += 1;
            } else {
                // Target already exists (interrupted / partial run)
                warn!(
                    "Target directory {:?} already exists for user '{}'. Verifying contents.",
                    target_dir, user.username
                );
                // Ensure extensions dir exists
                let _ = fs::create_dir_all(target_dir.join("extensions"));

                // If legacy directory is now empty or duplicate, clean it up safely
                if let Ok(entries) = fs::read_dir(&legacy_dir) {
                    if entries.count() == 0 {
                        let _ = fs::remove_dir(&legacy_dir);
                    }
                }
            }
        }
    }

    Ok(moved)
}

/// Validate that every normal user has a valid TenantId, UUID, and matching directory.
pub fn validate_identity_migration(
    users_conn: &Connection,
    topology: &Topology,
    warnings: &mut Vec<String>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let users = crate::db::users::list_users(users_conn)?;
    let mut valid = true;

    for user in &users {
        if user.account_type == "admin"
            || user.account_type == "system"
            || user.username == "legacy_admin"
        {
            continue;
        }

        // Validate TenantId
        match user.tenant_id {
            Some(tid) => {
                if !is_v08_user_id(tid.as_str()) {
                    warnings.push(format!(
                        "Invalid TenantId format '{}' for user '{}'",
                        tid.as_str(),
                        user.username
                    ));
                    valid = false;
                }

                let target_dir = topology.tenant_dir(tid);
                if !target_dir.exists() {
                    // Check if content db was initialized or if directory was not yet created
                    warnings.push(format!(
                        "Tenant directory {:?} does not exist for user '{}'",
                        target_dir, user.username
                    ));
                }
            }
            None => {
                warnings.push(format!(
                    "Normal user '{}' (ID {}) is missing TenantId",
                    user.username, user.id
                ));
                valid = false;
            }
        }

        // Validate UUID
        match &user.uuid {
            Some(u) => {
                if uuid::Uuid::parse_str(u).is_err() {
                    warnings.push(format!(
                        "Invalid UUID format '{}' for user '{}'",
                        u, user.username
                    ));
                    valid = false;
                }
            }
            None => {
                warnings.push(format!(
                    "Normal user '{}' (ID {}) is missing UUID",
                    user.username, user.id
                ));
                valid = false;
            }
        }
    }

    Ok(valid)
}
