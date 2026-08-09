use super::*;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BackupFileRow {
    pub filename: String,
    pub size_str: String,
    pub created_str: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BackupHistoryRow {
    pub id: String,
    pub backup_path: String,
    pub status: String,
    pub created_at: String,
    pub size_bytes: i64,
    pub error_message: Option<String>,
}

// GET /admin/backups
pub async fn backups_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let mut files = vec![];
    if let Ok(dir_entries) = std::fs::read_dir(&state.config.backup_dir) {
        for entry in dir_entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(filename) = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string())
                {
                    if filename.ends_with(".tar.gz") {
                        let meta = entry.metadata().unwrap();
                        let size_str = format_size(meta.len());
                        let created_str = meta
                            .created()
                            .ok()
                            .map(|c| {
                                let datetime: chrono::DateTime<chrono::Utc> = c.into();
                                datetime.format("%Y-%m-%d %H:%M:%S").to_string()
                            })
                            .unwrap_or_else(|| "-".to_string());
                        files.push(BackupFileRow {
                            filename,
                            size_str,
                            created_str,
                        });
                    }
                }
            }
        }
    }
    files.sort_by(|a, b| b.filename.cmp(&a.filename));

    let history = {
        let conn = state.system_db.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, backup_path, status, created_at, size_bytes, error_message FROM backup_history ORDER BY created_at DESC LIMIT 30;").unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok(BackupHistoryRow {
                    id: row.get(0)?,
                    backup_path: row.get(1)?,
                    status: row.get(2)?,
                    created_at: row.get(3)?,
                    size_bytes: row.get(4)?,
                    error_message: row.get(5)?,
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    };

    let csrf_token = generate_csrf_token(&session_id);

    let template = crate::templates::BackupsTemplate {
        admin_username: user.username,
        files,
        history,
        csrf_token,
        success: params.get("success").cloned(),
        error: params.get("error").cloned(),
    };

    template.into_response()
}

// POST /admin/backups/create
pub async fn backups_create_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let (_user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let form_csrf = form.get("csrf_token").cloned().unwrap_or_default();
    if !verify_csrf(&session_id, &form_csrf) {
        return Redirect::to("/admin/backups?error=Invalid CSRF token").into_response();
    }

    match crate::jobs::backup::perform_backup(&state.db, &state.config).await {
        Ok(path) => {
            let filename = std::path::Path::new(&path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("backup.tar.gz");
            Redirect::to(&format!(
                "/admin/backups?success=Backup created successfully: {}",
                filename
            ))
            .into_response()
        }
        Err(e) => Redirect::to(&format!(
            "/admin/backups?error=Failed to generate backup: {}",
            e
        ))
        .into_response(),
    }
}

// GET /admin/backups/download/:filename
pub async fn backups_download_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(filename): Path<String>,
) -> Response {
    if require_auth(&state, &jar).await.is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return StatusCode::BAD_REQUEST.into_response();
    }

    let backup_path = state.config.backup_dir.join(&filename);
    if !backup_path.exists() {
        return StatusCode::NOT_FOUND.into_response();
    }

    match std::fs::read(&backup_path) {
        Ok(bytes) => {
            let body = axum::body::Body::from(bytes);
            Response::builder()
                .header("content-type", "application/octet-stream")
                .header(
                    "content-disposition",
                    format!("attachment; filename=\"{}\"", filename),
                )
                .body(body)
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

// POST /admin/backups/delete/:filename
pub async fn backups_delete_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(filename): Path<String>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let form_csrf = form.get("csrf_token").cloned().unwrap_or_default();
    if !verify_csrf(&session_id, &form_csrf) {
        return Redirect::to("/admin/backups?error=Invalid CSRF token").into_response();
    }

    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Redirect::to("/admin/backups?error=Invalid filename").into_response();
    }

    let backup_path = state.config.backup_dir.join(&filename);
    if !backup_path.exists() {
        return Redirect::to("/admin/backups?error=Backup file not found").into_response();
    }

    match std::fs::remove_file(&backup_path) {
        Ok(_) => {
            let system_conn = state.system_db.lock().unwrap();
            let _ = crate::db::audit_events::write_audit_event(
                &system_conn,
                &user.username,
                "BACKUP_DELETE",
                "backup",
                &filename,
                None,
            );
            Redirect::to("/admin/backups?success=Backup archive deleted").into_response()
        }
        Err(e) => Redirect::to(&format!(
            "/admin/backups?error=Failed to delete backup file: {}",
            e
        ))
        .into_response(),
    }
}

// POST /admin/backups/restore
pub async fn backups_restore_post(
    State(state): State<AppState>,
    jar: CookieJar,
    mut multipart: axum::extract::Multipart,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let mut backup_bytes = vec![];
    let mut confirm_text = String::new();
    let mut csrf_token = String::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or_default().to_string();
        if name == "backup_file" {
            if let Ok(bytes) = field.bytes().await {
                backup_bytes = bytes.to_vec();
            }
        } else if name == "confirm_text" {
            if let Ok(text) = field.text().await {
                confirm_text = text.trim().to_string();
            }
        } else if name == "csrf_token" {
            if let Ok(text) = field.text().await {
                csrf_token = text.trim().to_string();
            }
        }
    }

    if !verify_csrf(&session_id, &csrf_token) {
        return Redirect::to("/admin/backups?error=Invalid CSRF token").into_response();
    }

    if confirm_text != "RESTORE" {
        return Redirect::to(
            "/admin/backups?error=Confirmation text mismatch. Please type RESTORE.",
        )
        .into_response();
    }

    if backup_bytes.is_empty() {
        return Redirect::to("/admin/backups?error=Backup file is empty or missing.")
            .into_response();
    }

    let temp_file_path = state.config.data_dir.join("temp-restore-upload.tar.gz");
    if let Err(e) = std::fs::write(&temp_file_path, &backup_bytes) {
        return Redirect::to(&format!(
            "/admin/backups?error=Failed to save uploaded file: {}",
            e
        ))
        .into_response();
    }

    match crate::cli::restore::run(
        temp_file_path.to_string_lossy().to_string(),
        None,
        state.config.clone(),
    )
    .await
    {
        Ok(_) => {
            let _ = std::fs::remove_file(&temp_file_path);
            let conn = state.users_db.lock().unwrap();
            let _ = conn.execute("DELETE FROM sessions;", []);

            let system_conn = state.system_db.lock().unwrap();
            let _ = crate::db::audit_events::write_audit_event(
                &system_conn,
                &user.username,
                "RESTORE_EXECUTION",
                "backup",
                "upload",
                None,
            );
            Redirect::to("/admin/login?success=Restore successful. Please log in again.")
                .into_response()
        }
        Err(e) => {
            let _ = std::fs::remove_file(&temp_file_path);
            Redirect::to(&format!("/admin/backups?error=Restore failed: {}", e)).into_response()
        }
    }
}
