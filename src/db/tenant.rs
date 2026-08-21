//! Tenant database access boundary.
//!
//! New tenant opens go through [`TenantId`]. Legacy integer row ids are used
//! only to look up a registered Core user, then resolved to a [`TenantLocation`].
//! Unknown ids must not create filesystem databases.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::db::topology::Topology;
use crate::error::AppError;
use crate::identity::TenantId;
use crate::models::TenantUser;

/// Open tenant SQLite connections (content, analytics, profile).
#[derive(Clone)]
pub struct UserDbs {
    pub content: Arc<Mutex<Connection>>,
    pub analytics: Arc<Mutex<Connection>>,
    pub profile: Arc<Mutex<Connection>>,
}

/// How a tenant database may be opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TenantOpenMode {
    /// Authenticated tenant user / API actor. Status must be `active`.
    Ordinary,
    /// Public slug/QR/gate resolution. User must exist and not be deleted.
    PublicContent,
    /// Core jobs / admin inspection. User must exist and not be deleted.
    /// Existing files only — will not create a database.
    CoreJob,
    /// Explicit provisioning after a Core user row was inserted.
    Provision,
}

/// Resolved tenant filesystem location.
///
/// `Id` is the frozen v0.8 path. `Legacy` is unmigrated v0.7 `users/<integer>/`
/// and exists only until Phase 3 directory migration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TenantLocation {
    Id(TenantId),
    Legacy(i64),
}

impl TenantLocation {
    pub fn cache_key(&self) -> String {
        match self {
            Self::Id(id) => id.as_str().to_string(),
            Self::Legacy(row_id) => format!("legacy:{row_id}"),
        }
    }

    pub fn dir(&self, topology: &Topology) -> Result<PathBuf, crate::db::topology::TopologyError> {
        match self {
            Self::Id(id) => Ok(topology.tenant_dir(*id)),
            Self::Legacy(row_id) => topology.user_dir_i64(*row_id),
        }
    }
}

pub fn location_for_user(user: &TenantUser) -> Result<TenantLocation, AppError> {
    if let Some(id) = user.tenant_id {
        return Ok(TenantLocation::Id(id));
    }
    if user.id <= 0 {
        return Err(AppError::NotFound("invalid user id".into()));
    }
    Ok(TenantLocation::Legacy(user.id))
}

pub fn status_allows_open(status: &str, mode: TenantOpenMode) -> bool {
    match mode {
        TenantOpenMode::Ordinary => status == "active",
        TenantOpenMode::PublicContent | TenantOpenMode::CoreJob | TenantOpenMode::Provision => {
            status != "deleted"
        }
    }
}

pub fn assert_may_open(user: &TenantUser, mode: TenantOpenMode) -> Result<(), AppError> {
    if !status_allows_open(&user.status, mode) {
        return Err(AppError::Unauthorized(format!(
            "tenant access denied for status '{}'",
            user.status
        )));
    }
    Ok(())
}

/// Open tenant DBs for a registered Core user (legacy row id compatibility).
pub fn open_for_row_id(
    users_conn: &Connection,
    topology: &Topology,
    system_db: &Arc<Mutex<Connection>>,
    pool: &mut std::collections::HashMap<String, UserDbs>,
    user_id: i64,
    mode: TenantOpenMode,
) -> Result<UserDbs, AppError> {
    let user = crate::db::users::get_user_by_id(users_conn, user_id)?
        .ok_or_else(|| AppError::NotFound(format!("user {user_id} not found")))?;
    assert_may_open(&user, mode)?;
    let location = location_for_user(&user)?;
    open_location(topology, system_db, pool, &location, mode)
}

/// Open tenant DBs by frozen TenantId. Unknown ids never create files.
pub fn open_for_tenant_id(
    users_conn: &Connection,
    topology: &Topology,
    system_db: &Arc<Mutex<Connection>>,
    pool: &mut std::collections::HashMap<String, UserDbs>,
    tenant_id: TenantId,
    mode: TenantOpenMode,
) -> Result<UserDbs, AppError> {
    let user = crate::db::users::get_user_by_tenant_id(users_conn, tenant_id)?
        .ok_or_else(|| AppError::NotFound(format!("tenant {tenant_id} not found")))?;
    assert_may_open(&user, mode)?;
    let location = location_for_user(&user)?;
    open_location(topology, system_db, pool, &location, mode)
}

pub fn open_location(
    topology: &Topology,
    system_db: &Arc<Mutex<Connection>>,
    pool: &mut std::collections::HashMap<String, UserDbs>,
    location: &TenantLocation,
    mode: TenantOpenMode,
) -> Result<UserDbs, AppError> {
    let key = location.cache_key();
    if let Some(dbs) = pool.get(&key) {
        return Ok(dbs.clone());
    }

    let user_dir = location
        .dir(topology)
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    let content_path = user_dir.join("content.db");
    let analytics_path = user_dir.join("analytics.db");
    let profile_path = user_dir.join("profile.db");

    let create = matches!(
        mode,
        TenantOpenMode::Provision | TenantOpenMode::Ordinary | TenantOpenMode::PublicContent
    );
    if !create && !content_path.exists() {
        return Err(AppError::NotFound(format!(
            "tenant database missing at {}",
            content_path.display()
        )));
    }
    if create {
        std::fs::create_dir_all(user_dir.join("extensions"))?;
    }

    let dbs = open_files(
        &content_path,
        &analytics_path,
        &profile_path,
        system_db,
        create,
    )?;
    pool.insert(key, dbs.clone());
    Ok(dbs)
}

fn open_files(
    content_path: &Path,
    analytics_path: &Path,
    profile_path: &Path,
    system_db: &Arc<Mutex<Connection>>,
    create: bool,
) -> Result<UserDbs, AppError> {
    if !create && !content_path.exists() {
        return Err(AppError::NotFound("content.db missing".into()));
    }

    let mut content_conn = Connection::open(content_path)?;
    let mut analytics_conn = Connection::open(analytics_path)?;
    let profile_conn = Connection::open(profile_path)?;

    crate::db::sqlite::enable_wal(&content_conn, "content")?;
    crate::db::sqlite::enable_wal(&analytics_conn, "analytics")?;
    crate::db::sqlite::enable_wal(&profile_conn, "profile")?;
    crate::db::sqlite::enable_foreign_keys(&content_conn, "content")?;
    crate::db::sqlite::enable_foreign_keys(&analytics_conn, "analytics")?;
    crate::db::sqlite::enable_foreign_keys(&profile_conn, "profile")?;

    crate::db::migrations::run_migrations(
        &mut content_conn,
        "content",
        crate::db::migrations::CONTENT_MIGRATIONS,
        Some(system_db),
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;
    crate::db::migrations::run_migrations(
        &mut analytics_conn,
        "analytics",
        crate::db::migrations::ANALYTICS_MIGRATIONS,
        Some(system_db),
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;

    profile_conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )?;

    Ok(UserDbs {
        content: Arc::new(Mutex::new(content_conn)),
        analytics: Arc::new(Mutex::new(analytics_conn)),
        profile: Arc::new(Mutex::new(profile_conn)),
    })
}

/// Job/helper path: registered user, existing content.db only, never create.
pub fn existing_content_path(
    users_conn: &Connection,
    topology: &Topology,
    user_id: i64,
) -> Result<PathBuf, rusqlite::Error> {
    let user = crate::db::users::get_user_by_id(users_conn, user_id)?.ok_or_else(|| {
        rusqlite::Error::InvalidPath(PathBuf::from(format!("unknown-user-{user_id}")))
    })?;
    if user.status == "deleted" {
        return Err(rusqlite::Error::InvalidPath(PathBuf::from("deleted-user")));
    }
    let loc = location_for_user(&user)
        .map_err(|e| rusqlite::Error::InvalidPath(PathBuf::from(e.to_string())))?;
    let dir = loc
        .dir(topology)
        .map_err(|e| rusqlite::Error::InvalidPath(PathBuf::from(e.to_string())))?;
    let path = dir.join("content.db");
    if !path.exists() {
        return Err(rusqlite::Error::InvalidPath(path));
    }
    Ok(path)
}

pub fn existing_analytics_path(
    users_conn: &Connection,
    topology: &Topology,
    user_id: i64,
) -> Result<PathBuf, rusqlite::Error> {
    let user = crate::db::users::get_user_by_id(users_conn, user_id)?.ok_or_else(|| {
        rusqlite::Error::InvalidPath(PathBuf::from(format!("unknown-user-{user_id}")))
    })?;
    if user.status == "deleted" {
        return Err(rusqlite::Error::InvalidPath(PathBuf::from("deleted-user")));
    }
    let loc = location_for_user(&user)
        .map_err(|e| rusqlite::Error::InvalidPath(PathBuf::from(e.to_string())))?;
    let dir = loc
        .dir(topology)
        .map_err(|e| rusqlite::Error::InvalidPath(PathBuf::from(e.to_string())))?;
    let path = dir.join("analytics.db");
    if !path.exists() {
        return Err(rusqlite::Error::InvalidPath(path));
    }
    Ok(path)
}
