use crate::db::topology::Topology;
use crate::identity::TenantId;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryIssueType {
    MissingTenant,
    MissingTarget,
    CorruptDatabase,
    AccessFailure,
    TrueOrphan,
    Conflict,
    StaleReservation,
    InvalidStatus,
    InvalidTargetType,
}

#[derive(Debug, Clone)]
pub struct RegistryIssue {
    pub slug: String,
    pub target_type: String,
    pub owner_tenant_id: String,
    pub database_path: PathBuf,
    pub target_id: String,
    pub issue_type: RegistryIssueType,
    pub description: String,
}

pub struct RegistryValidator;

impl RegistryValidator {
    /// Scans the v0.8 slug registries (`global_urls.db`, `global_landing_pages.db`, `reserved.db`)
    /// and returns a list of detected issues categorized per safety policies.
    pub fn scan(
        _system_conn: &Connection,
        users_conn: &Connection,
        data_dir: &Path,
        slug_filter: Option<&str>,
    ) -> Result<Vec<RegistryIssue>, Box<dyn std::error::Error>> {
        let topology = Topology::new(data_dir);
        let mut issues = Vec::new();

        let urls_path = topology.global_urls_db();
        let pages_path = topology.global_landing_pages_db();
        let reserved_path = topology.reserved_db();

        if !urls_path.exists() || !pages_path.exists() || !reserved_path.exists() {
            issues.push(RegistryIssue {
                slug: "*".to_string(),
                target_type: "system".to_string(),
                owner_tenant_id: "".to_string(),
                database_path: urls_path,
                target_id: "".to_string(),
                issue_type: RegistryIssueType::AccessFailure,
                description: "One or more v0.8 slug databases are missing from disk".to_string(),
            });
            return Ok(issues);
        }

        let urls_conn = Connection::open(&urls_path)?;
        let pages_conn = Connection::open(&pages_path)?;
        let reserved_conn = Connection::open(&reserved_path)?;

        // 1. Scan global_urls.db
        Self::scan_table(
            &urls_conn,
            users_conn,
            &reserved_conn,
            &pages_conn,
            &topology,
            "url",
            slug_filter,
            &mut issues,
        )?;

        // 2. Scan global_landing_pages.db
        Self::scan_table(
            &pages_conn,
            users_conn,
            &reserved_conn,
            &urls_conn,
            &topology,
            "page",
            slug_filter,
            &mut issues,
        )?;

        Ok(issues)
    }

    #[allow(clippy::too_many_arguments)]
    fn scan_table(
        conn: &Connection,
        users_conn: &Connection,
        reserved_conn: &Connection,
        other_conn: &Connection,
        topology: &Topology,
        target_type: &str,
        slug_filter: Option<&str>,
        issues: &mut Vec<RegistryIssue>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (query, params_vec) = if let Some(slug) = slug_filter {
            (
                format!(
                    "SELECT slug, owner_tenant_id, target_id, created_at, status FROM global_{}s WHERE slug = ?1;",
                    if target_type == "url" { "url" } else { "landing_page" }
                ),
                vec![slug.to_string()],
            )
        } else {
            (
                format!(
                    "SELECT slug, owner_tenant_id, target_id, created_at, status FROM global_{}s;",
                    if target_type == "url" {
                        "url"
                    } else {
                        "landing_page"
                    }
                ),
                vec![],
            )
        };

        let mut stmt = conn.prepare(&query)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(params_vec))?;

        while let Some(row) = rows.next()? {
            let slug: String = row.get(0)?;
            let owner_tenant_id_str: String = row.get(1)?;
            let target_id: String = row.get(2)?;
            let created_at_str: String = row.get(3)?;
            let status: String = row.get(4)?;

            let tenant_id_res = TenantId::parse(&owner_tenant_id_str);
            let content_db_path = match tenant_id_res {
                Ok(tid) => topology.tenant_dir(tid).join("content.db"),
                Err(_) => topology.users_dir().join("_invalid").join("content.db"),
            };

            // 1. Conflict with reserved.db
            let is_reserved: bool = reserved_conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM reserved_slugs WHERE slug = ?1);",
                    [&slug],
                    |r| r.get(0),
                )
                .unwrap_or(false);

            if is_reserved {
                issues.push(RegistryIssue {
                    slug: slug.clone(),
                    target_type: target_type.to_string(),
                    owner_tenant_id: owner_tenant_id_str.clone(),
                    database_path: topology.reserved_db(),
                    target_id: target_id.clone(),
                    issue_type: RegistryIssueType::Conflict,
                    description: format!("Slug '{}' conflicts with a reserved system route", slug),
                });
            }

            // 2. Conflict with the other slug database
            let other_table = if target_type == "url" {
                "global_landing_pages"
            } else {
                "global_urls"
            };
            let in_other: bool = other_conn
                .query_row(
                    &format!("SELECT EXISTS(SELECT 1 FROM {other_table} WHERE slug = ?1);"),
                    [&slug],
                    |r| r.get(0),
                )
                .unwrap_or(false);

            if in_other {
                issues.push(RegistryIssue {
                    slug: slug.clone(),
                    target_type: target_type.to_string(),
                    owner_tenant_id: owner_tenant_id_str.clone(),
                    database_path: conn.path().map(PathBuf::from).unwrap_or_default(),
                    target_id: target_id.clone(),
                    issue_type: RegistryIssueType::Conflict,
                    description: format!(
                        "Slug '{}' exists in both global_urls.db and global_landing_pages.db",
                        slug
                    ),
                });
            }

            // 3. Status check
            if status != "active"
                && status != "disabled"
                && status != "reserving"
                && status != "retired"
            {
                issues.push(RegistryIssue {
                    slug: slug.clone(),
                    target_type: target_type.to_string(),
                    owner_tenant_id: owner_tenant_id_str.clone(),
                    database_path: conn.path().map(PathBuf::from).unwrap_or_default(),
                    target_id: target_id.clone(),
                    issue_type: RegistryIssueType::InvalidStatus,
                    description: format!("Slug '{}' has invalid status '{}'", slug, status),
                });
            }

            // If retired, no active target check is needed
            if status == "retired" {
                continue;
            }

            // 4. Owner tenant check in users.db
            let tid = match tenant_id_res {
                Ok(t) => t,
                Err(_) => {
                    issues.push(RegistryIssue {
                        slug: slug.clone(),
                        target_type: target_type.to_string(),
                        owner_tenant_id: owner_tenant_id_str.clone(),
                        database_path: content_db_path,
                        target_id: target_id.clone(),
                        issue_type: RegistryIssueType::MissingTenant,
                        description: format!(
                            "Slug '{}' has invalid TenantId '{}'",
                            slug, owner_tenant_id_str
                        ),
                    });
                    continue;
                }
            };

            let owner_opt = crate::db::users::get_user_by_tenant_id(users_conn, tid)?;
            let owner_exists = match owner_opt {
                Some(ref u) => u.status != "deleted",
                None => false,
            };

            if !owner_exists {
                issues.push(RegistryIssue {
                    slug: slug.clone(),
                    target_type: target_type.to_string(),
                    owner_tenant_id: owner_tenant_id_str.clone(),
                    database_path: content_db_path.clone(),
                    target_id: target_id.clone(),
                    issue_type: RegistryIssueType::MissingTenant,
                    description: format!(
                        "Slug '{}' references missing or deleted owner tenant '{}'",
                        slug, owner_tenant_id_str
                    ),
                });
                continue;
            }

            // 5. Stale reservation check
            if status == "reserving" {
                if let Ok(created_at) = DateTime::parse_from_rfc3339(&created_at_str) {
                    let age = Utc::now().signed_duration_since(created_at.with_timezone(&Utc));
                    if age > chrono::Duration::try_minutes(15).unwrap_or_default() {
                        issues.push(RegistryIssue {
                            slug: slug.clone(),
                            target_type: target_type.to_string(),
                            owner_tenant_id: owner_tenant_id_str.clone(),
                            database_path: content_db_path.clone(),
                            target_id: target_id.clone(),
                            issue_type: RegistryIssueType::StaleReservation,
                            description: format!(
                                "Reserving slug '{}' has been stale for over 15 minutes",
                                slug
                            ),
                        });
                    }
                }
            }

            // 6. Target record check in tenant content DB
            if status == "active" || status == "disabled" {
                if !content_db_path.exists() {
                    issues.push(RegistryIssue {
                        slug: slug.clone(),
                        target_type: target_type.to_string(),
                        owner_tenant_id: owner_tenant_id_str.clone(),
                        database_path: content_db_path.clone(),
                        target_id: target_id.clone(),
                        issue_type: RegistryIssueType::TrueOrphan,
                        description: format!(
                            "Slug '{}' owner content database does not exist at {:?}",
                            slug, content_db_path
                        ),
                    });
                } else {
                    match Connection::open(&content_db_path) {
                        Ok(tenant_conn) => {
                            let exists = if target_type == "url" {
                                tenant_conn
                                    .query_row(
                                        "SELECT EXISTS(SELECT 1 FROM urls WHERE id = ?1);",
                                        [&target_id],
                                        |r| r.get(0),
                                    )
                                    .unwrap_or(false)
                            } else {
                                tenant_conn
                                    .query_row(
                                        "SELECT EXISTS(SELECT 1 FROM landing_pages WHERE id = ?1);",
                                        [&target_id],
                                        |r| r.get(0),
                                    )
                                    .unwrap_or(false)
                            };

                            if !exists {
                                issues.push(RegistryIssue {
                                    slug: slug.clone(),
                                    target_type: target_type.to_string(),
                                    owner_tenant_id: owner_tenant_id_str.clone(),
                                    database_path: content_db_path.clone(),
                                    target_id: target_id.clone(),
                                    issue_type: RegistryIssueType::MissingTarget,
                                    description: format!(
                                        "Slug '{}' (type: '{}', id: '{}') references missing target record in tenant content database",
                                        slug, target_type, target_id
                                    ),
                                });
                            }
                        }
                        Err(rusqlite::Error::SqliteFailure(err, _))
                            if err.code == rusqlite::ErrorCode::DatabaseCorrupt =>
                        {
                            issues.push(RegistryIssue {
                                slug: slug.clone(),
                                target_type: target_type.to_string(),
                                owner_tenant_id: owner_tenant_id_str.clone(),
                                database_path: content_db_path.clone(),
                                target_id: target_id.clone(),
                                issue_type: RegistryIssueType::CorruptDatabase,
                                description: format!(
                                    "Slug '{}' owner content database is corrupt at {:?}",
                                    slug, content_db_path
                                ),
                            });
                        }
                        Err(e) => {
                            issues.push(RegistryIssue {
                                slug: slug.clone(),
                                target_type: target_type.to_string(),
                                owner_tenant_id: owner_tenant_id_str.clone(),
                                database_path: content_db_path.clone(),
                                target_id: target_id.clone(),
                                issue_type: RegistryIssueType::AccessFailure,
                                description: format!(
                                    "Slug '{}' owner content database could not be accessed: {}",
                                    slug, e
                                ),
                            });
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
