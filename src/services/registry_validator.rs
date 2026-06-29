use rusqlite::Connection;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum RegistryIssueType {
    DuplicateSlug,
    InvalidTargetType,
    InvalidStatus,
    MissingOwner,
    MissingDatabase,
    MissingTarget,
    StaleReservation,
    TenantAdminHasIsolatedContent,
}

#[derive(Debug, Clone)]
pub struct RegistryIssue {
    pub slug: String,
    pub target_type: String,
    pub owner_user_id: i64,
    pub database_path: PathBuf,
    pub target_id: String,
    pub issue_type: RegistryIssueType,
    pub description: String,
}

pub struct RegistryValidator;

impl RegistryValidator {
    /// Scans the global_slugs registry and returns a list of detected issues.
    pub fn scan(
        system_conn: &Connection,
        users_conn: &Connection,
        data_dir: &Path,
        slug_filter: Option<&str>,
    ) -> Result<Vec<RegistryIssue>, Box<dyn std::error::Error>> {
        use chrono::{DateTime, Utc};
        let mut issues = Vec::new();

        // 1. Check duplicate slugs (only if not filtering by single slug)
        if slug_filter.is_none() {
            let total_count: i64 =
                system_conn.query_row("SELECT COUNT(*) FROM global_slugs;", [], |r| r.get(0))?;
            let distinct_count: i64 = system_conn.query_row(
                "SELECT COUNT(DISTINCT slug) FROM global_slugs;",
                [],
                |r| r.get(0),
            )?;
            if total_count != distinct_count {
                issues.push(RegistryIssue {
                    slug: "*".to_string(),
                    target_type: "system".to_string(),
                    owner_user_id: 0,
                    database_path: data_dir.join("admin/system.db"),
                    target_id: "".to_string(),
                    issue_type: RegistryIssueType::DuplicateSlug,
                    description: format!(
                        "Duplicate slugs found in global_slugs table (total rows: {}, distinct slugs: {})",
                        total_count, distinct_count
                    ),
                });
            }
        }

        // 2. Scan global slugs
        let (query, params_string) = if let Some(slug) = slug_filter {
            (
                "SELECT slug, owner_user_id, target_type, target_id, created_at, status FROM global_slugs WHERE slug = ?1;",
                vec![slug.to_string()],
            )
        } else {
            (
                "SELECT slug, owner_user_id, target_type, target_id, created_at, status FROM global_slugs;",
                vec![],
            )
        };

        let mut stmt = system_conn.prepare(query)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(params_string))?;

        while let Some(row) = rows.next()? {
            let slug: String = row.get(0)?;
            let owner_user_id: i64 = row.get(1)?;
            let target_type: String = row.get(2)?;
            let target_id: String = row.get(3)?;
            let created_at_str: String = row.get(4)?;
            let status: String = row.get(5)?;

            let content_db_path = if owner_user_id == 1 {
                data_dir.join("users").join("1").join("content.db")
            } else {
                data_dir
                    .join("users")
                    .join(owner_user_id.to_string())
                    .join("content.db")
            };

            // Target type check
            if target_type != "url" && target_type != "page" {
                issues.push(RegistryIssue {
                    slug: slug.clone(),
                    target_type: target_type.clone(),
                    owner_user_id,
                    database_path: content_db_path.clone(),
                    target_id: target_id.clone(),
                    issue_type: RegistryIssueType::InvalidTargetType,
                    description: format!(
                        "Slug '{}' has invalid target_type '{}'",
                        slug, target_type
                    ),
                });
            }

            // Status check
            if status != "active" && status != "disabled" && status != "reserving" {
                issues.push(RegistryIssue {
                    slug: slug.clone(),
                    target_type: target_type.clone(),
                    owner_user_id,
                    database_path: content_db_path.clone(),
                    target_id: target_id.clone(),
                    issue_type: RegistryIssueType::InvalidStatus,
                    description: format!("Slug '{}' has invalid status '{}'", slug, status),
                });
            }

            // Check owner
            let owner_exists: bool = users_conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM users WHERE id = ?1);",
                    [owner_user_id],
                    |r| r.get(0),
                )
                .unwrap_or(false);

            if !owner_exists {
                issues.push(RegistryIssue {
                    slug: slug.clone(),
                    target_type: target_type.clone(),
                    owner_user_id,
                    database_path: content_db_path.clone(),
                    target_id: target_id.clone(),
                    issue_type: RegistryIssueType::MissingOwner,
                    description: format!(
                        "Slug '{}' references missing owner user ID {}",
                        slug, owner_user_id
                    ),
                });
                continue;
            }

            // Stale warning check
            if status == "reserving" {
                if let Ok(created_at) = DateTime::parse_from_rfc3339(&created_at_str) {
                    let age = Utc::now().signed_duration_since(created_at.with_timezone(&Utc));
                    if age > chrono::Duration::try_minutes(15).unwrap_or_default() {
                        issues.push(RegistryIssue {
                            slug: slug.clone(),
                            target_type: target_type.clone(),
                            owner_user_id,
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

            // Check target record exists for active / disabled (and reserving with target_id)
            if status == "active"
                || status == "disabled"
                || (status == "reserving" && !target_id.is_empty())
            {
                if !content_db_path.exists() {
                    issues.push(RegistryIssue {
                        slug: slug.clone(),
                        target_type: target_type.clone(),
                        owner_user_id,
                        database_path: content_db_path.clone(),
                        target_id: target_id.clone(),
                        issue_type: RegistryIssueType::MissingDatabase,
                        description: format!(
                            "Slug '{}' owner content database does not exist at {:?}",
                            slug, content_db_path
                        ),
                    });
                } else {
                    match Connection::open(&content_db_path) {
                        Ok(conn) => {
                            let exists = if target_type == "url" {
                                conn.query_row(
                                    "SELECT EXISTS(SELECT 1 FROM urls WHERE id = ?1);",
                                    [&target_id],
                                    |r| r.get(0),
                                )
                                .unwrap_or(false)
                            } else if target_type == "page" {
                                conn.query_row(
                                    "SELECT EXISTS(SELECT 1 FROM landing_pages WHERE id = ?1);",
                                    [&target_id],
                                    |r| r.get(0),
                                )
                                .unwrap_or(false)
                            } else {
                                false
                            };

                            if !exists {
                                issues.push(RegistryIssue {
                                    slug: slug.clone(),
                                    target_type: target_type.clone(),
                                    owner_user_id,
                                    database_path: content_db_path.clone(),
                                    target_id: target_id.clone(),
                                    issue_type: RegistryIssueType::MissingTarget,
                                    description: format!("Slug '{}' (type: '{}', id: '{}') references missing target record in owner's content database", slug, target_type, target_id),
                                });
                            }
                        }
                        Err(e) => {
                            issues.push(RegistryIssue {
                                slug: slug.clone(),
                                target_type: target_type.clone(),
                                owner_user_id,
                                database_path: content_db_path.clone(),
                                target_id: target_id.clone(),
                                issue_type: RegistryIssueType::MissingDatabase,
                                description: format!(
                                    "Slug '{}' owner content database could not be opened: {}",
                                    slug, e
                                ),
                            });
                        }
                    }
                }
            }
        }

        // 3. Admin Content Reverse Consistency Check (Legacy DB)
        // Check if tenant databases contain content for admin users incorrectly (isolated admin content)
        if slug_filter.is_none() {
            let mut stmt = users_conn.prepare(
                "SELECT id, username FROM users WHERE account_type = 'admin' AND id != 1;",
            )?;
            let mut admin_rows = stmt.query([])?;
            while let Some(row) = admin_rows.next()? {
                let id: i64 = row.get(0)?;
                let username: String = row.get(1)?;
                let tenant_db_path = data_dir
                    .join("users")
                    .join(id.to_string())
                    .join("content.db");

                if tenant_db_path.exists() {
                    if let Ok(tenant_conn) = Connection::open(&tenant_db_path) {
                        let url_count: i64 = tenant_conn
                            .query_row("SELECT COUNT(*) FROM urls;", [], |r| r.get(0))
                            .unwrap_or(0);
                        let page_count: i64 = tenant_conn
                            .query_row("SELECT COUNT(*) FROM landing_pages;", [], |r| r.get(0))
                            .unwrap_or(0);

                        if url_count > 0 || page_count > 0 {
                            issues.push(RegistryIssue {
                                slug: "*".to_string(),
                                target_type: "system".to_string(),
                                owner_user_id: id,
                                database_path: tenant_db_path.clone(),
                                target_id: "".to_string(),
                                issue_type: RegistryIssueType::TenantAdminHasIsolatedContent,
                                description: format!("Admin user '{}' (ID {}) has content in isolated tenant DB ({} URLs, {} pages). Admin content should be in legacy DB 1.", username, id, url_count, page_count),
                            });
                        }
                    }
                }
            }
        }

        Ok(issues)
    }
}
