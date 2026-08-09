use super::*;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct JobHistoryRow {
    pub id: String,
    pub job_name: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HealthCheckRow {
    pub id: String,
    pub object_type: String,
    pub object_id: String,
    pub checked_at: String,
    pub status_code: Option<i64>,
    pub error_message: Option<String>,
    pub is_healthy: i64,
}

// GET /admin/health
pub async fn health_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let mut db_reports = vec![];
    if let Ok(r) =
        crate::db::sqlite::collect_health_report(&state.admin_db.lock().unwrap(), "admin")
    {
        db_reports.push(r);
    }
    if let Ok(r) =
        crate::db::sqlite::collect_health_report(&state.system_db.lock().unwrap(), "system")
    {
        db_reports.push(r);
    }
    if let Ok(r) =
        crate::db::sqlite::collect_health_report(&state.users_db.lock().unwrap(), "users")
    {
        db_reports.push(r);
    }

    let system_db_path = state.config.data_dir.join("admin").join("system.db");
    let users_db_path = state.config.data_dir.join("admin").join("users.db");
    let admin_db_path = state.config.data_dir.join("admin").join("admin.db");

    let system_db_size = format_size(
        std::fs::metadata(&system_db_path)
            .map(|m| m.len())
            .unwrap_or(0),
    );
    let users_db_size = format_size(
        std::fs::metadata(&users_db_path)
            .map(|m| m.len())
            .unwrap_or(0),
    );
    let admin_db_size = format_size(
        std::fs::metadata(&admin_db_path)
            .map(|m| m.len())
            .unwrap_or(0),
    );

    let users_dir = state.config.data_dir.join("users");
    let tenants_db_size = format_size(get_dir_size(&users_dir).unwrap_or(0));
    let total_data_size = format_size(get_dir_size(&state.config.data_dir).unwrap_or(0));

    let job_history = {
        let conn = state.system_db.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, job_name, status, started_at, finished_at, error_message FROM job_history ORDER BY started_at DESC LIMIT 20;").unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok(JobHistoryRow {
                    id: row.get(0)?,
                    job_name: row.get(1)?,
                    status: row.get(2)?,
                    started_at: row.get(3)?,
                    finished_at: row.get(4)?,
                    error_message: row.get(5)?,
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    };

    let health_checks = {
        let conn = state.system_db.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, object_type, object_id, checked_at, status_code, error_message, is_healthy FROM health_checks ORDER BY checked_at DESC LIMIT 20;").unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok(HealthCheckRow {
                    id: row.get(0)?,
                    object_type: row.get(1)?,
                    object_id: row.get(2)?,
                    checked_at: row.get(3)?,
                    status_code: row.get(4)?,
                    error_message: row.get(5)?,
                    is_healthy: row.get(6)?,
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    };

    let csrf_token = generate_csrf_token(&session_id);

    let (registry_errors, registry_warnings) = {
        let system_conn = state.system_db.lock().unwrap();
        let users_conn = state.users_db.lock().unwrap();
        match crate::services::registry_validator::RegistryValidator::scan(
            &system_conn,
            &users_conn,
            &state.config.data_dir,
            None,
        ) {
            Ok(issues) => {
                let mut errors = Vec::new();
                let mut warnings = Vec::new();
                for issue in issues {
                    use crate::services::registry_validator::RegistryIssueType;
                    match issue.issue_type {
                        RegistryIssueType::StaleReservation
                        | RegistryIssueType::TenantAdminHasIsolatedContent => {
                            warnings.push(format!(
                                "Warning for slug {}: {}",
                                issue.slug, issue.description
                            ));
                        }
                        _ => {
                            errors.push(format!(
                                "Error for slug {}: {}",
                                issue.slug, issue.description
                            ));
                        }
                    }
                }
                (errors, warnings)
            }
            Err(e) => (vec![format!("Failed to run registry scan: {}", e)], vec![]),
        }
    };

    let template = crate::templates::HealthTemplate {
        admin_username: user.username,
        db_reports,
        total_data_size,
        system_db_size,
        users_db_size,
        admin_db_size,
        tenants_db_size,
        job_history,
        health_checks,
        registry_errors,
        registry_warnings,
        csrf_token,
        success: params.get("success").cloned(),
        error: params.get("error").cloned(),
    };

    template.into_response()
}
