//! Post-restore filesystem layout normalization for multi-tenant BZOD data dirs.
//!
//! Extracted from admin restore handlers so path moves are testable without HTTP.

use std::path::{Path, PathBuf};
use tracing::warn;

use crate::db::topology::{Topology, LEGACY_ADMIN_USER_KEY};

/// Move flat legacy DB files into multi-tenant paths after tarball extract.
///
/// Layout:
/// - `admin.db` / `system.db` / `users.db` (+ wal/shm) → `{data_dir}/admin/`
/// - `content.db` / `analytics.db` (+ wal/shm) → `{data_dir}/users/1/`
/// - `{data_dir}/slugs/` is created empty if missing (v0.8 topology)
pub fn normalize_restored_layout(data_dir: &Path) -> std::io::Result<()> {
    let topology = Topology::new(data_dir);
    let admin_dir = topology.admin_dir();
    let users_1_dir = topology.legacy_admin_dir();
    std::fs::create_dir_all(&admin_dir)?;
    std::fs::create_dir_all(&users_1_dir)?;
    std::fs::create_dir_all(topology.slugs_dir())?;

    let admin_files = [
        "admin.db",
        "admin.db-wal",
        "admin.db-shm",
        "system.db",
        "system.db-wal",
        "system.db-shm",
        "users.db",
        "users.db-wal",
        "users.db-shm",
    ];
    for f in admin_files {
        let src = data_dir.join(f);
        if src.exists() {
            let dst = admin_dir.join(f);
            if let Err(e) = std::fs::rename(&src, &dst) {
                warn!(
                    file = f,
                    error = %e,
                    "failed to move restored admin file into admin/"
                );
                return Err(e);
            }
        }
    }

    let content_files = [
        "content.db",
        "content.db-wal",
        "content.db-shm",
        "analytics.db",
        "analytics.db-wal",
        "analytics.db-shm",
    ];
    for f in content_files {
        let src = data_dir.join(f);
        if src.exists() {
            let dst = users_1_dir.join(f);
            if let Err(e) = std::fs::rename(&src, &dst) {
                warn!(
                    file = f,
                    error = %e,
                    "failed to move restored content file into users/1/"
                );
                return Err(e);
            }
        }
    }

    Ok(())
}

/// Paths used when reopening connections after restore.
#[derive(Debug, Clone)]
pub struct RestoredDbPaths {
    pub admin: PathBuf,
    pub system: PathBuf,
    pub users: PathBuf,
    pub content: PathBuf,
    pub analytics: PathBuf,
}

impl RestoredDbPaths {
    pub fn from_data_dir(data_dir: &Path) -> Self {
        let topology = Topology::new(data_dir);
        Self {
            admin: topology.admin_db(),
            system: topology.system_db(),
            users: topology.users_registry_db(),
            content: topology
                .content_db(LEGACY_ADMIN_USER_KEY)
                .expect("legacy admin user key is valid"),
            analytics: topology
                .analytics_db(LEGACY_ADMIN_USER_KEY)
                .expect("legacy admin user key is valid"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn moves_flat_files_into_tenant_layout() {
        let dir = std::env::temp_dir().join(format!("bzod_layout_{}", uuid::Uuid::new_v4()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("admin.db"), b"a").unwrap();
        fs::write(dir.join("system.db"), b"s").unwrap();
        fs::write(dir.join("users.db"), b"u").unwrap();
        fs::write(dir.join("content.db"), b"c").unwrap();
        fs::write(dir.join("analytics.db"), b"an").unwrap();

        normalize_restored_layout(&dir).unwrap();

        assert!(dir.join("admin/admin.db").exists());
        assert!(dir.join("admin/system.db").exists());
        assert!(dir.join("admin/users.db").exists());
        assert!(dir.join("users/1/content.db").exists());
        assert!(dir.join("users/1/analytics.db").exists());
        assert!(dir.join("slugs").is_dir());
        assert!(!dir.join("admin.db").exists());
        assert!(!dir.join("content.db").exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
