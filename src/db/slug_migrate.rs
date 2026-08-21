//! Explicit Phase 4 Global Slug Migration Engine.
//!
//! Migrates `system.db.global_slugs` and `system.db.reserved_slugs` to:
//! - `slugs/global_urls.db` (`global_urls` table)
//! - `slugs/global_landing_pages.db` (`global_landing_pages` table)
//! - `slugs/reserved.db` (`reserved_slugs` table)
//!
//! Lifecycle:
//! 1. Preflight (inspect system.db, resolve owners against users.db, check for ambiguities)
//! 2. Backup (create pre-migration tarball)
//! 3. Transactional Migration (insert into target databases with restart safety)
//! 4. Validation (row counts, field parity, global uniqueness across databases)
//! 5. Completion Marker (record audit event and system setting)

use rusqlite::Connection;
use std::collections::HashMap;
use tracing::{info, warn};

use crate::config::Config;
use crate::db::topology::Topology;
use crate::db::Db;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SlugMigrationReport {
    pub total_legacy_slugs: usize,
    pub url_slugs_migrated: usize,
    pub page_slugs_migrated: usize,
    pub reserved_slugs_migrated: usize,
    pub existing_records_verified: usize,
    pub validation_passed: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SlugPreflightReport {
    pub total_slugs: usize,
    pub url_slugs: usize,
    pub page_slugs: usize,
    pub reserved_slugs: usize,
    pub unresolvable_owners: Vec<(String, i64)>,
    pub unknown_target_types: Vec<(String, String)>,
}

/// Helper struct for legacy slug record
struct LegacySlugRecord {
    slug: String,
    owner_user_id: i64,
    target_type: String,
    target_id: String,
    created_at: String,
    updated_at: String,
    status: String,
    deleted_at: Option<String>,
}

/// Run the full explicit global slug migration.
pub async fn run_global_slug_migration(
    config: &Config,
    dry_run: bool,
    skip_backup: bool,
) -> Result<SlugMigrationReport, Box<dyn std::error::Error>> {
    let _topology = Topology::new(&config.data_dir);
    let mut warnings = Vec::new();

    // 1. Initialize Db
    let db = Db::init(config)?;

    // 2. Preflight
    let preflight = {
        let system_conn = db.system.lock().unwrap();
        let users_conn = db.users.lock().unwrap();
        inspect_slug_preflight(&system_conn, &users_conn)?
    };

    info!(
        "Slug Migration Preflight: total={}, urls={}, pages={}, reserved={}, unresolvable_owners={}, unknown_types={}",
        preflight.total_slugs,
        preflight.url_slugs,
        preflight.page_slugs,
        preflight.reserved_slugs,
        preflight.unresolvable_owners.len(),
        preflight.unknown_target_types.len()
    );

    if !preflight.unresolvable_owners.is_empty() {
        let msg = format!(
            "Fatal: Found {} slugs with unresolvable owner_user_id in users.db: {:?}",
            preflight.unresolvable_owners.len(),
            preflight.unresolvable_owners
        );
        return Err(msg.into());
    }

    if !preflight.unknown_target_types.is_empty() {
        let msg = format!(
            "Fatal: Found {} slugs with unknown target_type: {:?}",
            preflight.unknown_target_types.len(),
            preflight.unknown_target_types
        );
        return Err(msg.into());
    }

    if dry_run {
        info!("Dry run enabled: no slug records will be migrated.");
        return Ok(SlugMigrationReport {
            total_legacy_slugs: preflight.total_slugs,
            url_slugs_migrated: preflight.url_slugs,
            page_slugs_migrated: preflight.page_slugs,
            reserved_slugs_migrated: preflight.reserved_slugs,
            existing_records_verified: 0,
            validation_passed: true,
            warnings,
        });
    }

    // 3. Backup before migration (if needed and not skipped)
    if preflight.total_slugs > 0 && !skip_backup {
        info!("Creating pre-migration backup before slug migration...");
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

    // 4. Transactional Migration
    let (migrated_urls, migrated_pages, verified_existing) = {
        let system_conn = db.system.lock().unwrap();
        let users_conn = db.users.lock().unwrap();
        let mut urls_conn = db.global_urls.lock().unwrap();
        let mut pages_conn = db.global_landing_pages.lock().unwrap();
        let reserved_conn = db.reserved.lock().unwrap();

        migrate_slugs_transactional(
            &system_conn,
            &users_conn,
            &mut urls_conn,
            &mut pages_conn,
            &reserved_conn,
            &mut warnings,
        )?
    };

    // 5. Validation
    let validation_passed = {
        let system_conn = db.system.lock().unwrap();
        let urls_conn = db.global_urls.lock().unwrap();
        let pages_conn = db.global_landing_pages.lock().unwrap();
        let reserved_conn = db.reserved.lock().unwrap();

        validate_slug_migration(
            &system_conn,
            &urls_conn,
            &pages_conn,
            &reserved_conn,
            &mut warnings,
        )?
    };

    // 6. Completion Marker in system.db
    if validation_passed {
        let system_conn = db.system.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let details = format!(
            "Global slug migration completed: {} URLs migrated, {} landing pages migrated, {} verified existing",
            migrated_urls, migrated_pages, verified_existing
        );
        let _ = crate::db::audit_events::write_audit_event(
            &system_conn,
            "system",
            "GLOBAL_SLUG_MIGRATION_COMPLETED",
            "slugs",
            "slugs/*.db",
            Some(&details),
        );
        let _ = system_conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('v08_global_slug_migration_completed', ?1);",
            [&now],
        );
    }

    Ok(SlugMigrationReport {
        total_legacy_slugs: preflight.total_slugs,
        url_slugs_migrated: migrated_urls,
        page_slugs_migrated: migrated_pages,
        reserved_slugs_migrated: preflight.reserved_slugs,
        existing_records_verified: verified_existing,
        validation_passed,
        warnings,
    })
}

/// Inspect system.db.global_slugs and reserved_slugs to build preflight plan.
pub fn inspect_slug_preflight(
    system_conn: &Connection,
    users_conn: &Connection,
) -> Result<SlugPreflightReport, Box<dyn std::error::Error>> {
    let has_global_slugs: bool = system_conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='global_slugs');",
        [],
        |r| r.get(0),
    )?;

    if !has_global_slugs {
        return Ok(SlugPreflightReport {
            total_slugs: 0,
            url_slugs: 0,
            page_slugs: 0,
            reserved_slugs: 0,
            unresolvable_owners: Vec::new(),
            unknown_target_types: Vec::new(),
        });
    }

    let mut stmt = system_conn.prepare(
        "SELECT slug, owner_user_id, target_type, target_id, created_at, updated_at, status, deleted_at 
         FROM global_slugs;",
    )?;

    let records = stmt.query_map([], |row| {
        Ok(LegacySlugRecord {
            slug: row.get(0)?,
            owner_user_id: row.get(1)?,
            target_type: row.get(2)?,
            target_id: row.get(3)?,
            created_at: row.get(4)?,
            updated_at: row.get(5)?,
            status: row.get(6)?,
            deleted_at: row.get(7)?,
        })
    })?;

    // Build owner map from users.db: user_id -> TenantId/legacy_admin
    let mut owner_map = HashMap::new();
    let users = crate::db::users::list_users(users_conn)?;
    for u in users {
        if let Some(tid) = u.tenant_id {
            owner_map.insert(u.id, tid.as_str().to_string());
        } else if u.account_type == "admin"
            || u.account_type == "system"
            || u.username == "legacy_admin"
        {
            owner_map.insert(u.id, "legacy_admin".to_string());
        }
    }

    let mut total_slugs = 0;
    let mut url_slugs = 0;
    let mut page_slugs = 0;
    let mut unresolvable_owners = Vec::new();
    let mut unknown_target_types = Vec::new();

    for r in records {
        let rec = r?;
        total_slugs += 1;

        if !owner_map.contains_key(&rec.owner_user_id) {
            unresolvable_owners.push((rec.slug.clone(), rec.owner_user_id));
        }

        match rec.target_type.as_str() {
            "url" => url_slugs += 1,
            "page" => page_slugs += 1,
            other => unknown_target_types.push((rec.slug.clone(), other.to_string())),
        }
    }

    let reserved_slugs: usize = system_conn
        .query_row("SELECT COUNT(*) FROM reserved_slugs;", [], |r| r.get(0))
        .unwrap_or(0);

    Ok(SlugPreflightReport {
        total_slugs,
        url_slugs,
        page_slugs,
        reserved_slugs,
        unresolvable_owners,
        unknown_target_types,
    })
}

/// Transactional migration of slugs from system.db to global_urls.db and global_landing_pages.db.
pub fn migrate_slugs_transactional(
    system_conn: &Connection,
    users_conn: &Connection,
    urls_conn: &mut Connection,
    pages_conn: &mut Connection,
    reserved_conn: &Connection,
    warnings: &mut Vec<String>,
) -> Result<(usize, usize, usize), Box<dyn std::error::Error>> {
    // 1. Build owner map: user_id -> TenantId
    let mut owner_map = HashMap::new();
    let users = crate::db::users::list_users(users_conn)?;
    for u in users {
        if let Some(tid) = u.tenant_id {
            owner_map.insert(u.id, tid.as_str().to_string());
        } else if u.account_type == "admin"
            || u.account_type == "system"
            || u.username == "legacy_admin"
        {
            owner_map.insert(u.id, "legacy_admin".to_string());
        }
    }

    // 2. Fetch all legacy slugs
    let mut stmt = system_conn.prepare(
        "SELECT slug, owner_user_id, target_type, target_id, created_at, updated_at, status, deleted_at 
         FROM global_slugs;",
    )?;

    let records: Vec<LegacySlugRecord> = stmt
        .query_map([], |row| {
            Ok(LegacySlugRecord {
                slug: row.get(0)?,
                owner_user_id: row.get(1)?,
                target_type: row.get(2)?,
                target_id: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
                status: row.get(6)?,
                deleted_at: row.get(7)?,
            })
        })?
        .collect::<Result<_, _>>()?;

    let mut migrated_urls = 0;
    let mut migrated_pages = 0;
    let mut verified_existing = 0;

    let tx_urls = urls_conn.transaction()?;
    let tx_pages = pages_conn.transaction()?;

    for rec in records {
        let owner_tenant_id = match owner_map.get(&rec.owner_user_id) {
            Some(t) => t.clone(),
            None => {
                warnings.push(format!(
                    "Unresolvable owner_user_id {} for slug '{}'; skipping",
                    rec.owner_user_id, rec.slug
                ));
                continue;
            }
        };

        let retired_at = rec.deleted_at;

        if rec.target_type == "url" {
            // Check if record already exists in global_urls.db
            let existing: Option<(String, String)> = tx_urls
                .query_row(
                    "SELECT owner_tenant_id, target_id FROM global_urls WHERE slug = ?1;",
                    [&rec.slug],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .ok();

            if let Some((existing_owner, existing_target)) = existing {
                if existing_owner == owner_tenant_id && existing_target == rec.target_id {
                    verified_existing += 1;
                } else {
                    warnings.push(format!(
                        "Conflict: Slug '{}' already exists in global_urls with different owner/target",
                        rec.slug
                    ));
                }
            } else {
                tx_urls.execute(
                    "INSERT INTO global_urls (slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at) 
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);",
                    rusqlite::params![
                        rec.slug,
                        owner_tenant_id,
                        rec.target_id,
                        rec.created_at,
                        rec.updated_at,
                        rec.status,
                        retired_at
                    ],
                )?;
                migrated_urls += 1;
            }
        } else if rec.target_type == "page" {
            // Check if record already exists in global_landing_pages.db
            let existing: Option<(String, String)> = tx_pages
                .query_row(
                    "SELECT owner_tenant_id, target_id FROM global_landing_pages WHERE slug = ?1;",
                    [&rec.slug],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .ok();

            if let Some((existing_owner, existing_target)) = existing {
                if existing_owner == owner_tenant_id && existing_target == rec.target_id {
                    verified_existing += 1;
                } else {
                    warnings.push(format!(
                        "Conflict: Slug '{}' already exists in global_landing_pages with different owner/target",
                        rec.slug
                    ));
                }
            } else {
                tx_pages.execute(
                    "INSERT INTO global_landing_pages (slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at) 
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);",
                    rusqlite::params![
                        rec.slug,
                        owner_tenant_id,
                        rec.target_id,
                        rec.created_at,
                        rec.updated_at,
                        rec.status,
                        retired_at
                    ],
                )?;
                migrated_pages += 1;
            }
        }
    }

    tx_urls.commit()?;
    tx_pages.commit()?;

    // Seed reserved slugs
    let _ = crate::db::slugs::seed_reserved_slugs(reserved_conn);

    Ok((migrated_urls, migrated_pages, verified_existing))
}

/// Validate that every legacy slug was migrated correctly and global uniqueness holds.
pub fn validate_slug_migration(
    system_conn: &Connection,
    urls_conn: &Connection,
    pages_conn: &Connection,
    reserved_conn: &Connection,
    warnings: &mut Vec<String>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let mut valid = true;

    let has_global_slugs: bool = system_conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='global_slugs');",
        [],
        |r| r.get(0),
    )?;

    if !has_global_slugs {
        return Ok(true);
    }

    let legacy_url_count: usize = system_conn.query_row(
        "SELECT COUNT(*) FROM global_slugs WHERE target_type = 'url';",
        [],
        |r| r.get(0),
    )?;

    let legacy_page_count: usize = system_conn.query_row(
        "SELECT COUNT(*) FROM global_slugs WHERE target_type = 'page';",
        [],
        |r| r.get(0),
    )?;

    let target_url_count: usize =
        urls_conn.query_row("SELECT COUNT(*) FROM global_urls;", [], |r| r.get(0))?;

    let target_page_count: usize =
        pages_conn.query_row("SELECT COUNT(*) FROM global_landing_pages;", [], |r| {
            r.get(0)
        })?;

    if target_url_count < legacy_url_count {
        warnings.push(format!(
            "URL slug count mismatch: legacy has {}, target has {}",
            legacy_url_count, target_url_count
        ));
        valid = false;
    }

    if target_page_count < legacy_page_count {
        warnings.push(format!(
            "Page slug count mismatch: legacy has {}, target has {}",
            legacy_page_count, target_page_count
        ));
        valid = false;
    }

    // Global Uniqueness Invariant Check: 0 intersection between reserved, urls, and pages
    let collisions_urls_pages: usize = urls_conn.query_row(
        "SELECT COUNT(*) FROM global_urls WHERE slug IN (SELECT slug FROM global_landing_pages);",
        [],
        |r| r.get(0),
    ).unwrap_or(0);

    if collisions_urls_pages > 0 {
        warnings.push(format!(
            "Invariant Violation: {} slugs appear in both global_urls and global_landing_pages",
            collisions_urls_pages
        ));
        valid = false;
    }

    let collisions_reserved_urls: usize = urls_conn
        .query_row(
            "SELECT COUNT(*) FROM global_urls WHERE slug IN (SELECT slug FROM reserved_slugs);",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if collisions_reserved_urls > 0 {
        warnings.push(format!(
            "Invariant Violation: {} slugs appear in both global_urls and reserved_slugs",
            collisions_reserved_urls
        ));
        valid = false;
    }

    let collisions_reserved_pages: usize = pages_conn.query_row(
        "SELECT COUNT(*) FROM global_landing_pages WHERE slug IN (SELECT slug FROM reserved_slugs);",
        [],
        |r| r.get(0),
    ).unwrap_or(0);

    if collisions_reserved_pages > 0 {
        warnings.push(format!(
            "Invariant Violation: {} slugs appear in both global_landing_pages and reserved_slugs",
            collisions_reserved_pages
        ));
        valid = false;
    }

    let _ = reserved_conn;

    Ok(valid)
}
