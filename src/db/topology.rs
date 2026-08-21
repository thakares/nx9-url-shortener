//! Frozen v0.8.0 database topology under a configurable physical root.
//!
//! The logical layout is:
//!
//! ```text
//! <data_dir>/
//! ├── admin/{admin,system,users}.db
//! ├── slugs/{global_urls,global_landing_pages,reserved}.db
//! └── users/<user-key>/{profile,content,analytics}.db
//!     └── extensions/<extension>/<extension>.db
//! ```
//!
//! `<data_dir>` is `Config.data_dir` (env `DATA_DIR`, default `./data`).
//! It is not renamed to `database/`. Production Docker, CasaOS, and
//! `deploy.sh` bind this physical root.

use std::path::{Path, PathBuf};

use crate::identity::TenantId;

/// Directory name of the migration-era `legacy_admin` tenant (`users.db` id = 1).
pub const LEGACY_ADMIN_USER_KEY: &str = "1";

/// Frozen first-party extension names. There is no runtime plugin loader.
pub const FIRST_PARTY_EXTENSIONS: &[&str] = &["cv", "certificates", "documents", "portfolio"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopologyError {
    InvalidUserDir { name: String },
    InvalidExtension { name: String },
}

impl std::fmt::Display for TopologyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidUserDir { name } => {
                write!(f, "invalid tenant directory name: {name:?}")
            }
            Self::InvalidExtension { name } => {
                write!(f, "invalid extension name: {name:?}")
            }
        }
    }
}

impl std::error::Error for TopologyError {}

/// Authoritative resolver for every BZOD database path.
#[derive(Clone, Debug)]
pub struct Topology {
    root: PathBuf,
}

impl Topology {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn admin_dir(&self) -> PathBuf {
        self.root.join("admin")
    }

    pub fn slugs_dir(&self) -> PathBuf {
        self.root.join("slugs")
    }

    pub fn users_dir(&self) -> PathBuf {
        self.root.join("users")
    }

    pub fn admin_db(&self) -> PathBuf {
        self.admin_dir().join("admin.db")
    }

    pub fn system_db(&self) -> PathBuf {
        self.admin_dir().join("system.db")
    }

    pub fn users_registry_db(&self) -> PathBuf {
        self.admin_dir().join("users.db")
    }

    pub fn global_urls_db(&self) -> PathBuf {
        self.slugs_dir().join("global_urls.db")
    }

    pub fn global_landing_pages_db(&self) -> PathBuf {
        self.slugs_dir().join("global_landing_pages.db")
    }

    pub fn reserved_db(&self) -> PathBuf {
        self.slugs_dir().join("reserved.db")
    }

    /// Pre-multi-tenant files that may still exist at the physical root.
    pub fn legacy_flat_admin_db(&self) -> PathBuf {
        self.root.join("admin.db")
    }

    pub fn legacy_flat_system_db(&self) -> PathBuf {
        self.root.join("system.db")
    }

    pub fn legacy_flat_users_db(&self) -> PathBuf {
        self.root.join("users.db")
    }

    pub fn legacy_flat_content_db(&self) -> PathBuf {
        self.root.join("content.db")
    }

    pub fn legacy_flat_analytics_db(&self) -> PathBuf {
        self.root.join("analytics.db")
    }

    pub fn legacy_admin_dir(&self) -> PathBuf {
        self.users_dir().join(LEGACY_ADMIN_USER_KEY)
    }

    /// Frozen tenant directory: `users/<12-hex-TenantId>/`.
    pub fn tenant_dir(&self, tenant_id: TenantId) -> PathBuf {
        self.users_dir().join(tenant_id.as_str())
    }

    pub fn tenant_content_db(&self, tenant_id: TenantId) -> PathBuf {
        self.tenant_dir(tenant_id).join("content.db")
    }

    pub fn tenant_analytics_db(&self, tenant_id: TenantId) -> PathBuf {
        self.tenant_dir(tenant_id).join("analytics.db")
    }

    pub fn tenant_profile_db(&self, tenant_id: TenantId) -> PathBuf {
        self.tenant_dir(tenant_id).join("profile.db")
    }

    pub fn user_dir(&self, user_key: &str) -> Result<PathBuf, TopologyError> {
        if !is_valid_user_dir_name(user_key) {
            return Err(TopologyError::InvalidUserDir {
                name: user_key.to_string(),
            });
        }
        Ok(self.users_dir().join(user_key))
    }

    pub fn user_dir_i64(&self, user_id: i64) -> Result<PathBuf, TopologyError> {
        self.user_dir(&user_id.to_string())
    }

    pub fn content_db(&self, user_key: &str) -> Result<PathBuf, TopologyError> {
        Ok(self.user_dir(user_key)?.join("content.db"))
    }

    pub fn analytics_db(&self, user_key: &str) -> Result<PathBuf, TopologyError> {
        Ok(self.user_dir(user_key)?.join("analytics.db"))
    }

    pub fn profile_db(&self, user_key: &str) -> Result<PathBuf, TopologyError> {
        Ok(self.user_dir(user_key)?.join("profile.db"))
    }

    pub fn content_db_i64(&self, user_id: i64) -> Result<PathBuf, TopologyError> {
        self.content_db(&user_id.to_string())
    }

    pub fn analytics_db_i64(&self, user_id: i64) -> Result<PathBuf, TopologyError> {
        self.analytics_db(&user_id.to_string())
    }

    pub fn profile_db_i64(&self, user_id: i64) -> Result<PathBuf, TopologyError> {
        self.profile_db(&user_id.to_string())
    }

    pub fn extensions_dir(&self, user_key: &str) -> Result<PathBuf, TopologyError> {
        Ok(self.user_dir(user_key)?.join("extensions"))
    }

    pub fn extensions_dir_i64(&self, user_id: i64) -> Result<PathBuf, TopologyError> {
        self.extensions_dir(&user_id.to_string())
    }

    pub fn extension_db(&self, user_key: &str, extension: &str) -> Result<PathBuf, TopologyError> {
        if !is_valid_extension_name(extension) {
            return Err(TopologyError::InvalidExtension {
                name: extension.to_string(),
            });
        }
        Ok(self
            .extensions_dir(user_key)?
            .join(extension)
            .join(format!("{extension}.db")))
    }

    /// Create `admin/`, `slugs/`, and `users/` under the physical root.
    pub fn ensure_core_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(self.admin_dir())?;
        std::fs::create_dir_all(self.slugs_dir())?;
        std::fs::create_dir_all(self.users_dir())?;
        Ok(())
    }

    /// Create a tenant directory and its `extensions/` folder.
    pub fn ensure_user_dirs(&self, user_key: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let dir = self.user_dir(user_key)?;
        std::fs::create_dir_all(&dir)?;
        std::fs::create_dir_all(dir.join("extensions"))?;
        Ok(dir)
    }

    pub fn ensure_user_dirs_i64(
        &self,
        user_id: i64,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        self.ensure_user_dirs(&user_id.to_string())
    }
}

/// Tenant directory names: 12 lowercase hex (v0.8) or a positive decimal id (v0.7).
pub fn is_valid_user_dir_name(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." {
        return false;
    }
    if name
        .as_bytes()
        .iter()
        .any(|b| *b == b'/' || *b == b'\\' || *b == 0 || *b == b'.')
    {
        return false;
    }
    if is_v08_user_id(name) {
        return true;
    }
    is_legacy_integer_user_id(name)
}

pub fn is_v08_user_id(name: &str) -> bool {
    name.len() == 12 && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub fn is_legacy_integer_user_id(name: &str) -> bool {
    if name.is_empty() || name.as_bytes()[0] == b'0' {
        return false;
    }
    name.bytes().all(|b| b.is_ascii_digit()) && name.parse::<i64>().map(|n| n > 0).unwrap_or(false)
}

/// First-party extension directory names: `^[a-z][a-z0-9_]{0,31}$`.
pub fn is_valid_extension_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    name.len() <= 32
        && name
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_root_is_not_renamed_to_database() {
        let t = Topology::new("/var/lib/bzod/data");
        assert_eq!(t.root(), Path::new("/var/lib/bzod/data"));
        assert!(t.admin_dir().ends_with("data/admin"));
        assert!(t.slugs_dir().ends_with("data/slugs"));
        assert!(t.users_dir().ends_with("data/users"));
        assert!(!t.root().ends_with("database"));
    }

    #[test]
    fn frozen_core_paths() {
        let t = Topology::new("/app/data");
        assert_eq!(t.admin_db(), PathBuf::from("/app/data/admin/admin.db"));
        assert_eq!(t.system_db(), PathBuf::from("/app/data/admin/system.db"));
        assert_eq!(
            t.users_registry_db(),
            PathBuf::from("/app/data/admin/users.db")
        );
        assert_eq!(
            t.global_urls_db(),
            PathBuf::from("/app/data/slugs/global_urls.db")
        );
        assert_eq!(
            t.global_landing_pages_db(),
            PathBuf::from("/app/data/slugs/global_landing_pages.db")
        );
        assert_eq!(
            t.reserved_db(),
            PathBuf::from("/app/data/slugs/reserved.db")
        );
    }

    #[test]
    fn tenant_paths_legacy_integer_and_v08_hex() {
        let t = Topology::new("/app/data");
        assert_eq!(
            t.content_db_i64(2).unwrap(),
            PathBuf::from("/app/data/users/2/content.db")
        );
        let tid = crate::identity::TenantId::parse("a1b2c3d4e5f6").unwrap();
        assert_eq!(
            t.tenant_content_db(tid),
            PathBuf::from("/app/data/users/a1b2c3d4e5f6/content.db")
        );
        assert_eq!(
            t.extension_db("a1b2c3d4e5f6", "cv").unwrap(),
            PathBuf::from("/app/data/users/a1b2c3d4e5f6/extensions/cv/cv.db")
        );
    }

    #[test]
    fn rejects_path_traversal_and_forged_names() {
        let t = Topology::new("/app/data");
        assert!(t.user_dir("..").is_err());
        assert!(t.user_dir("../2").is_err());
        assert!(t.user_dir("2/../3").is_err());
        assert!(t.user_dir("2/foo").is_err());
        assert!(t.user_dir("-1").is_err());
        assert!(t.user_dir("0").is_err());
        assert!(t.user_dir("01").is_err());
        assert!(t.user_dir("").is_err());
        assert!(t.user_dir("ABCDEFABCDEF").is_err());
        assert!(t.content_db_i64(-5).is_err());
        assert!(t.content_db_i64(0).is_err());
        assert!(t.extension_db("2", "../cv").is_err());
        assert!(t.extension_db("2", "cv/db").is_err());
        assert!(t.extension_db("2", "").is_err());
        assert!(t.extension_db("2", "CV").is_err());
    }

    #[test]
    fn first_party_extension_names_are_valid() {
        for name in FIRST_PARTY_EXTENSIONS {
            assert!(is_valid_extension_name(name), "{name}");
        }
    }
}
