use crate::analytics::queue::AnalyticsQueue;
use crate::config::Config;
use crate::db::Db;
use crate::identity::TenantId;
use rusqlite::{Connection, OptionalExtension};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub use crate::db::tenant::{TenantOpenMode, UserDbs};

#[derive(Clone)]
pub struct AppState {
    pub admin_db: Arc<Mutex<Connection>>,
    pub system_db: Arc<Mutex<Connection>>,
    pub users_db: Arc<Mutex<Connection>>,
    pub user_dbs: Arc<Mutex<HashMap<String, UserDbs>>>,
    pub db: Db,
    pub config: Config,
    pub analytics_queue: AnalyticsQueue,
    pub start_time: Instant,
}

impl AppState {
    /// Compatibility opener: Core `users.id` must refer to a registered,
    /// non-deleted user. Unknown ids never create a database.
    pub fn get_user_dbs(&self, user_id: i64) -> Result<UserDbs, crate::error::AppError> {
        self.get_user_dbs_with_mode(user_id, TenantOpenMode::PublicContent)
    }

    pub fn get_user_dbs_with_mode(
        &self,
        user_id: i64,
        mode: TenantOpenMode,
    ) -> Result<UserDbs, crate::error::AppError> {
        let users = crate::utils::lock_db(&self.users_db, "users_db")?;
        let mut pool = crate::utils::lock_db(&self.user_dbs, "user_dbs")?;
        crate::db::tenant::open_for_row_id(
            &users,
            &self.db.topology,
            &self.system_db,
            &mut pool,
            user_id,
            mode,
        )
    }

    /// Frozen tenant opener. Unknown TenantId never creates a database.
    pub fn open_tenant(
        &self,
        tenant_id: TenantId,
        mode: TenantOpenMode,
    ) -> Result<UserDbs, crate::error::AppError> {
        let users = crate::utils::lock_db(&self.users_db, "users_db")?;
        let mut pool = crate::utils::lock_db(&self.user_dbs, "user_dbs")?;
        crate::db::tenant::open_for_tenant_id(
            &users,
            &self.db.topology,
            &self.system_db,
            &mut pool,
            tenant_id,
            mode,
        )
    }

    pub fn lookup_slug(
        &self,
        slug: &str,
    ) -> Result<Option<crate::db::slugs::ResolvedSlugInfo>, crate::error::AppError> {
        let urls_conn = crate::utils::lock_db(&self.db.global_urls, "global_urls")?;
        let pages_conn =
            crate::utils::lock_db(&self.db.global_landing_pages, "global_landing_pages")?;
        if let Some(info) = crate::db::slugs::lookup_slug(&urls_conn, &pages_conn, slug)? {
            return Ok(Some(info));
        }

        // Fallback to system.db.global_slugs if table exists (backward compatibility during transition)
        let system_conn = crate::utils::lock_db(&self.system_db, "system_db")?;
        let has_table: bool = system_conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='global_slugs');",
                [],
                |r| r.get(0),
            )
            .unwrap_or(false);

        if has_table {
            let row_opt: Option<(i64, String, String, String)> = system_conn
                .query_row(
                    "SELECT owner_user_id, target_type, target_id, status FROM global_slugs WHERE slug = ?1;",
                    [slug],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()?;

            if let Some((owner_id, target_type, target_id, status)) = row_opt {
                let tt = match target_type.as_str() {
                    "page" => crate::db::slugs::SlugTargetType::LandingPage,
                    _ => crate::db::slugs::SlugTargetType::Url,
                };
                return Ok(Some(crate::db::slugs::ResolvedSlugInfo {
                    slug: slug.to_string(),
                    owner_tenant_id: owner_id.to_string(),
                    target_type: tt,
                    target_id,
                    created_at: String::new(),
                    updated_at: String::new(),
                    status,
                    retired_at: None,
                }));
            }
        }

        Ok(None)
    }

    pub fn open_slug_owner(
        &self,
        owner_tenant_id: &str,
        mode: TenantOpenMode,
    ) -> Result<UserDbs, crate::error::AppError> {
        if owner_tenant_id == "legacy_admin" || owner_tenant_id == "1" {
            return self.get_user_dbs_with_mode(1, mode);
        }
        if let Ok(tid) = TenantId::parse(owner_tenant_id) {
            return self.open_tenant(tid, mode);
        }
        if let Ok(uid) = owner_tenant_id.parse::<i64>() {
            return self.get_user_dbs_with_mode(uid, mode);
        }
        Err(crate::error::AppError::NotFound(format!(
            "invalid owner identity: {}",
            owner_tenant_id
        )))
    }

    pub fn db_compact(&self) -> Result<(), crate::error::AppError> {
        crate::utils::lock_db(&self.admin_db, "admin_db")?.execute("VACUUM;", [])?;
        crate::utils::lock_db(&self.system_db, "system_db")?.execute("VACUUM;", [])?;
        crate::utils::lock_db(&self.users_db, "users_db")?.execute("VACUUM;", [])?;
        crate::utils::lock_db(&self.db.global_urls, "global_urls")?.execute("VACUUM;", [])?;
        crate::utils::lock_db(&self.db.global_landing_pages, "global_landing_pages")?
            .execute("VACUUM;", [])?;
        crate::utils::lock_db(&self.db.reserved, "reserved")?.execute("VACUUM;", [])?;
        Ok(())
    }
}
