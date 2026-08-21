use super::*;

#[derive(Deserialize)]
pub struct UserSettingsQuery {
    pub success: Option<String>,
    pub error: Option<String>,
}

pub async fn user_settings_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<UserSettingsQuery>,
) -> Response {
    let (user, session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let csrf_token = generate_csrf_token(&session_id);
    let template = crate::templates::UserSettingsTemplate {
        admin_username: user.username.clone(),
        username: user.username,
        csrf_token,
        success: query.success,
        error: query.error,
    };

    template.into_response()
}

pub async fn user_change_password_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Form(form): Form<ChangePasswordForm>,
) -> Response {
    let (user, session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to("/user/settings?error=Invalid CSRF token").into_response();
    }

    if form.current_password.trim().is_empty() || form.new_password.trim().len() < 8 {
        return Redirect::to("/user/settings?error=Password must be at least 8 characters")
            .into_response();
    }

    let conn = state.users_db.lock().unwrap();
    if !verify_password(&form.current_password, &user.password_hash) {
        let _ = write_audit_log(
            &state.admin_db.lock().unwrap(),
            &state,
            &user.username,
            "USER_PASSWORD_CHANGE_FAIL",
            Some("user"),
            Some(&user.id.to_string()),
            Some(&get_client_ip(&headers, connect_info)),
            headers.get("user-agent").and_then(|h| h.to_str().ok()),
        );
        return Redirect::to("/user/settings?error=Incorrect current password").into_response();
    }

    let new_hash = match hash_password(&form.new_password) {
        Ok(h) => h,
        Err(_) => return Redirect::to("/user/settings?error=Hashing error").into_response(),
    };

    match conn.execute(
        "UPDATE users SET password_hash = ?1 WHERE id = ?2;",
        params![new_hash, user.id],
    ) {
        Ok(_) => {
            let _ = write_audit_log(
                &state.admin_db.lock().unwrap(),
                &state,
                &user.username,
                "USER_PASSWORD_CHANGED",
                Some("user"),
                Some(&user.id.to_string()),
                Some(&get_client_ip(&headers, connect_info)),
                headers.get("user-agent").and_then(|h| h.to_str().ok()),
            );
            Redirect::to("/user/settings?success=Password updated successfully").into_response()
        }
        Err(e) => Redirect::to(&format!(
            "/user/settings?error=Failed to update password: {}",
            e
        ))
        .into_response(),
    }
}

pub async fn user_download_backup(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
) -> Response {
    let (user, _) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let ip = get_client_ip(&headers, connect_info);
    let user_dir = match crate::db::tenant::location_for_user(&user).and_then(|loc| {
        loc.dir(&state.db.topology)
            .map_err(|e| crate::error::AppError::BadRequest(e.to_string()))
    }) {
        Ok(p) => p,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    };

    let mut buffer = Vec::new();
    let res = {
        let enc = GzEncoder::new(&mut buffer, Compression::default());
        let mut tar = Builder::new(enc);
        let files = vec!["content.db", "analytics.db", "profile.db"];
        let mut add_err = None;
        for f in files {
            let path = user_dir.join(f);
            if path.exists() {
                if let Err(e) = tar.append_path_with_name(&path, f) {
                    add_err = Some(e);
                    break;
                }
            }
        }
        match add_err {
            Some(e) => Err(e),
            None => match tar.into_inner().and_then(|encoder| encoder.finish()) {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            },
        }
    };

    match res {
        Ok(_) => {
            let _ = write_audit_log(
                &state.admin_db.lock().unwrap(),
                &state,
                &user.username,
                "USER_BACKUP",
                Some("user"),
                Some(&user.id.to_string()),
                Some(&ip),
                headers.get("user-agent").and_then(|h| h.to_str().ok()),
            );
            let date_str = Utc::now().format("%Y-%m-%d").to_string();
            let filename = format!("user-{}-bzod-backup.tar.gz", date_str);
            (
                StatusCode::OK,
                [
                    ("Content-Type", "application/gzip"),
                    (
                        "Content-Disposition",
                        &format!("attachment; filename=\"{}\"", filename),
                    ),
                ],
                buffer,
            )
                .into_response()
        }
        Err(e) => {
            Redirect::to(&format!("/user/settings?error=Backup failed: {}", e)).into_response()
        }
    }
}

pub async fn user_restore_backup_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    mut multipart: axum::extract::Multipart,
) -> Response {
    let (user, session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let ip = get_client_ip(&headers, connect_info);
    let mut file_bytes = Vec::new();
    let mut confirm_text = String::new();
    let mut csrf_token = String::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "backup_file" {
            if let Ok(bytes) = field.bytes().await {
                file_bytes = bytes.to_vec();
            }
        } else if name == "confirm_text" {
            if let Ok(text) = field.text().await {
                confirm_text = text.trim().to_string();
            }
        } else if name == "csrf_token" {
            if let Ok(token) = field.text().await {
                csrf_token = token.trim().to_string();
            }
        }
    }

    if !verify_csrf(&session_id, &csrf_token) {
        return Redirect::to("/user/settings?error=Invalid CSRF token").into_response();
    }
    if confirm_text != "RESTORE" {
        return Redirect::to("/user/settings?error=Confirmation text must be exactly 'RESTORE'")
            .into_response();
    }
    if file_bytes.is_empty() {
        return Redirect::to("/user/settings?error=No backup file uploaded").into_response();
    }

    let temp_file_path =
        std::env::temp_dir().join(format!("bzod_user_restore_{}.tar.gz", uuid::Uuid::new_v4()));
    if let Err(e) = std::fs::write(&temp_file_path, &file_bytes) {
        return Redirect::to(&format!(
            "/user/settings?error=Failed to write temp file: {}",
            e
        ))
        .into_response();
    }

    let user_dir = match crate::db::tenant::location_for_user(&user).and_then(|loc| {
        loc.dir(&state.db.topology)
            .map_err(|e| crate::error::AppError::BadRequest(e.to_string()))
    }) {
        Ok(p) => p,
        Err(_) => {
            return Redirect::to("/user/settings?error=Invalid user directory").into_response()
        }
    };

    let temp_unpack_dir =
        std::env::temp_dir().join(format!("bzod_restore_unpack_{}", uuid::Uuid::new_v4()));
    if let Err(e) = std::fs::create_dir_all(&temp_unpack_dir) {
        let _ = std::fs::remove_file(&temp_file_path);
        return Redirect::to(&format!(
            "/user/settings?error=Failed to create temp dir: {}",
            e
        ))
        .into_response();
    }

    let restore_res: Result<(), Box<dyn std::error::Error>> = (|| {
        let file = File::open(&temp_file_path)?;
        let tar_gz = GzDecoder::new(file);
        let mut archive = tar::Archive::new(tar_gz);
        archive.unpack(&temp_unpack_dir)?;

        let target_tenant_id = user.tenant_id.ok_or("User is missing assigned TenantId")?;
        let temp_content_db = temp_unpack_dir.join("content.db");
        if !temp_content_db.exists() {
            return Err("Backup is missing content.db".into());
        }

        let content_conn = rusqlite::Connection::open(&temp_content_db)?;

        let mut urls = Vec::new();
        if let Ok(mut stmt) = content_conn.prepare("SELECT code, id, created_at, status FROM urls;")
        {
            if let Ok(rows) = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            }) {
                urls = rows.filter_map(|r| r.ok()).collect();
            }
        }

        let mut pages = Vec::new();
        if let Ok(mut stmt) =
            content_conn.prepare("SELECT code, id, created_at, state FROM landing_pages;")
        {
            if let Ok(rows) = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            }) {
                pages = rows.filter_map(|r| r.ok()).collect();
            }
        }

        let urls_conn = state.db.global_urls.lock().unwrap();
        let pages_conn = state.db.global_landing_pages.lock().unwrap();

        for (slug, _, _, _) in &urls {
            if let Ok(Some(existing_owner)) = urls_conn
                .query_row(
                    "SELECT owner_tenant_id FROM global_urls WHERE slug = ?1;",
                    [slug],
                    |r| r.get::<_, String>(0),
                )
                .optional()
            {
                if existing_owner != target_tenant_id.as_str() {
                    return Err(format!("Slug collision on URL /{}", slug).into());
                }
            }
        }

        for (slug, _, _, _) in &pages {
            if let Ok(Some(existing_owner)) = pages_conn
                .query_row(
                    "SELECT owner_tenant_id FROM global_landing_pages WHERE slug = ?1;",
                    [slug],
                    |r| r.get::<_, String>(0),
                )
                .optional()
            {
                if existing_owner != target_tenant_id.as_str() {
                    return Err(format!("Slug collision on Landing Page /{}", slug).into());
                }
            }
        }

        let _ = urls_conn.execute(
            "DELETE FROM global_urls WHERE owner_tenant_id = ?1;",
            [target_tenant_id.as_str()],
        );
        let _ = pages_conn.execute(
            "DELETE FROM global_landing_pages WHERE owner_tenant_id = ?1;",
            [target_tenant_id.as_str()],
        );

        let now = chrono::Utc::now().to_rfc3339();
        for (slug, target_id, created_at, status) in urls {
            let global_status = if status == "dead" {
                "disabled"
            } else {
                "active"
            };
            let _ = urls_conn.execute(
                "INSERT OR REPLACE INTO global_urls (slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at) 
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL);",
                rusqlite::params![slug, target_tenant_id.as_str(), target_id, created_at, now, global_status],
            );
        }

        for (slug, target_id, created_at, state) in pages {
            let global_status = if state == "published" {
                "active"
            } else {
                "disabled"
            };
            let _ = pages_conn.execute(
                "INSERT OR REPLACE INTO global_landing_pages (slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at) 
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL);",
                rusqlite::params![slug, target_tenant_id.as_str(), target_id, created_at, now, global_status],
            );
        }

        Ok(())
    })();

    let _ = std::fs::remove_file(&temp_file_path);

    match restore_res {
        Ok(_) => {
            // Success! Copy unpacked files from temp_unpack_dir to user_dir
            if let Err(e) = std::fs::create_dir_all(&user_dir) {
                let _ = std::fs::remove_dir_all(&temp_unpack_dir);
                return Redirect::to(&format!(
                    "/user/settings?error=Failed to create user directory: {}",
                    e
                ))
                .into_response();
            }

            for file_name in &["content.db", "analytics.db", "profile.db"] {
                let src = temp_unpack_dir.join(file_name);
                if src.exists() {
                    let dst = user_dir.join(file_name);
                    if let Err(e) = std::fs::copy(&src, &dst) {
                        let _ = std::fs::remove_dir_all(&temp_unpack_dir);
                        return Redirect::to(&format!(
                            "/user/settings?error=Failed to copy database: {}",
                            e
                        ))
                        .into_response();
                    }
                }
            }
            let _ = std::fs::remove_dir_all(&temp_unpack_dir);

            // Reconcile quotas
            let users_conn = state.users_db.lock().unwrap();
            if let Ok(content_conn) = rusqlite::Connection::open(user_dir.join("content.db")) {
                let _ =
                    crate::db::users::reconcile_user_quotas(&users_conn, user.id, &content_conn);
            }

            let mut pool = state.user_dbs.lock().unwrap();
            if let Ok(loc) = crate::db::tenant::location_for_user(&user) {
                pool.remove(&loc.cache_key());
            }
            let _ = write_audit_log(
                &state.admin_db.lock().unwrap(),
                &state,
                &user.username,
                "USER_RESTORE",
                Some("user"),
                Some(&user.id.to_string()),
                Some(&ip),
                headers.get("user-agent").and_then(|h| h.to_str().ok()),
            );
            Redirect::to("/user/settings?success=User database restored successfully")
                .into_response()
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&temp_unpack_dir);
            Redirect::to(&format!("/user/settings?error=Restore failed: {}", e)).into_response()
        }
    }
}

// GET /admin/settings
#[derive(Deserialize)]
pub struct SettingsQuery {
    pub success: Option<String>,
    pub error: Option<String>,
}

pub async fn settings_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<SettingsQuery>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let api_keys = {
        let conn = state.admin_db.lock().unwrap();
        list_api_keys(&conn, &user.id).unwrap_or_default()
    };

    let data_retention = {
        let conn = state.admin_db.lock().unwrap();
        get_config(&conn, "retention_days")
            .unwrap_or(None)
            .unwrap_or_else(|| {
                state
                    .config
                    .data_retention_days
                    .map(|d| d.to_string())
                    .unwrap_or_else(|| "unlimited".to_string())
            })
    };

    let csrf_token = generate_csrf_token(&session_id);

    let template = crate::templates::SettingsTemplate {
        admin_username: user.username,
        api_keys,
        data_retention,
        csrf_token,
        success: query.success,
        error: query.error,
    };

    template.into_response()
}

#[derive(Deserialize)]
pub struct ChangePasswordForm {
    pub current_password: String,
    pub new_password: String,
    pub csrf_token: String,
}

// POST /admin/settings/password
pub async fn change_password_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Form(form): Form<ChangePasswordForm>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to("/admin/settings?error=Invalid CSRF token").into_response();
    }

    let ip = get_client_ip(&headers, connect_info);

    let conn = state.admin_db.lock().unwrap();
    if !verify_password(&form.current_password, &user.password_hash) {
        let _ = write_audit_log(
            &conn,
            &state,
            &user.username,
            "PASSWORD_CHANGE_FAIL",
            Some("user"),
            Some(&user.id),
            Some(&ip),
            headers.get("user-agent").and_then(|h| h.to_str().ok()),
        );
        return Redirect::to("/admin/settings?error=Incorrect current password").into_response();
    }

    let new_hash = match hash_password(&form.new_password) {
        Ok(h) => h,
        Err(_) => return Redirect::to("/admin/settings?error=Hashing error").into_response(),
    };

    let res = conn.execute(
        "UPDATE users SET password_hash = ?1 WHERE id = ?2;",
        params![new_hash, user.id],
    );
    match res {
        Ok(_) => {
            let _ = write_audit_log(
                &conn,
                &state,
                &user.username,
                "PASSWORD_CHANGE_SUCCESS",
                Some("user"),
                Some(&user.id),
                Some(&ip),
                headers.get("user-agent").and_then(|h| h.to_str().ok()),
            );
            Redirect::to("/admin/settings?success=Password updated successfully").into_response()
        }
        Err(e) => Redirect::to(&format!(
            "/admin/settings?error=Failed to update password: {}",
            e
        ))
        .into_response(),
    }
}

#[derive(Deserialize)]
pub struct RetentionForm {
    pub retention: String,
    pub csrf_token: String,
}

// POST /admin/settings/retention
pub async fn change_retention_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Form(form): Form<RetentionForm>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to("/admin/settings?error=Invalid CSRF token").into_response();
    }

    let ip = get_client_ip(&headers, connect_info);

    let conn = state.admin_db.lock().unwrap();
    match set_config(&conn, "retention_days", &form.retention) {
        Ok(_) => {
            let _ = write_audit_log(
                &conn,
                &state,
                &user.username,
                "RETENTION_POLICY_CHANGED",
                Some("config"),
                Some("retention_days"),
                Some(&ip),
                headers.get("user-agent").and_then(|h| h.to_str().ok()),
            );
            Redirect::to("/admin/settings?success=Retention policy saved").into_response()
        }
        Err(e) => {
            Redirect::to(&format!("/admin/settings?error=Database error: {}", e)).into_response()
        }
    }
}

// POST /admin/settings/compact
pub async fn compact_db_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
) -> Response {
    let (user, _session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let ip = get_client_ip(&headers, connect_info);

    match state.db_compact() {
        Ok(_) => {
            let conn = state.admin_db.lock().unwrap();
            let _ = write_audit_log(
                &conn,
                &state,
                &user.username,
                "DATABASE_COMPACTION",
                Some("system"),
                Some("all_dbs"),
                Some(&ip),
                headers.get("user-agent").and_then(|h| h.to_str().ok()),
            );
            Redirect::to("/admin/settings?success=Database files compacted successfully")
                .into_response()
        }
        Err(e) => {
            Redirect::to(&format!("/admin/settings?error=Failed to compact: {}", e)).into_response()
        }
    }
}

// GET /admin/settings/backup
pub async fn download_backup(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
) -> Response {
    let (user, _) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let ip = get_client_ip(&headers, connect_info);

    // Create tar.gz in memory
    let mut buffer = Vec::new();
    let res = {
        let enc = GzEncoder::new(&mut buffer, Compression::default());
        let mut tar = Builder::new(enc);

        let files = vec![
            ("admin.db", state.config.data_dir.join("admin/admin.db")),
            ("system.db", state.config.data_dir.join("admin/system.db")),
            ("users.db", state.config.data_dir.join("admin/users.db")),
            (
                "content.db",
                state.config.data_dir.join("users/1/content.db"),
            ),
            (
                "analytics.db",
                state.config.data_dir.join("users/1/analytics.db"),
            ),
        ];

        let mut add_err = None;
        let mut manifest_files = Vec::new();

        for (name, path) in files {
            if path.exists() {
                if let Err(e) = tar.append_path_with_name(&path, name) {
                    add_err = Some(e);
                    break;
                }
                manifest_files.push(name.to_string());
            }
        }

        if add_err.is_none() {
            let manifest = serde_json::json!({
                "created_at": chrono::Utc::now().to_rfc3339(),
                "type": "legacy_flat_backup",
                "files_included": manifest_files,
                "note": "Multi-tenant databases flattened for backward compatibility.",
            });
            let manifest_str = manifest.to_string();
            let mut header = tar::Header::new_gnu();
            header.set_size(manifest_str.len() as u64);
            header.set_cksum();
            if let Err(e) =
                tar.append_data(&mut header, "backup_manifest.json", manifest_str.as_bytes())
            {
                add_err = Some(e);
            }
        }

        match add_err {
            Some(e) => Err(e),
            None => match tar.into_inner().and_then(|encoder| encoder.finish()) {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            },
        }
    };

    match res {
        Ok(_) => {
            {
                let conn = state.admin_db.lock().unwrap();
                let _ = write_audit_log(
                    &conn,
                    &state,
                    &user.username,
                    "DATABASE_BACKUP",
                    Some("system"),
                    Some("tarball"),
                    Some(&ip),
                    headers.get("user-agent").and_then(|h| h.to_str().ok()),
                );
            }

            let date_str = Utc::now().format("%Y-%m-%d").to_string();
            let filename = format!("{}-bzod-backup.tar.gz", date_str);

            (
                StatusCode::OK,
                [
                    ("Content-Type", "application/gzip"),
                    (
                        "Content-Disposition",
                        &format!("attachment; filename=\"{}\"", filename),
                    ),
                ],
                buffer,
            )
                .into_response()
        }
        Err(e) => {
            Redirect::to(&format!("/admin/settings?error=Backup failed: {}", e)).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct BulkQrExportForm {
    pub format: String,
    pub csrf_token: String,
}

// POST /admin/settings/bulk-qr
pub async fn bulk_qr_export_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Form(form): Form<BulkQrExportForm>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to("/admin/settings?error=Invalid CSRF token").into_response();
    }

    let ip = get_client_ip(&headers, connect_info);

    let urls = {
        let conn = state.db.global_urls.lock().unwrap();
        let mut stmt = match conn
            .prepare("SELECT slug, target_id FROM global_urls WHERE status = 'active' LIMIT 500;")
        {
            Ok(s) => s,
            Err(_) => return Redirect::to("/admin/settings?error=Database error").into_response(),
        };
        let rows = stmt.query_map([], |row| {
            let code: String = row.get(0)?;
            let target_id: String = row.get(1)?;
            Ok(crate::models::Url {
                id: target_id,
                code: code.clone(),
                destination: format!(
                    "{}/{}",
                    state.config.base_url.clone().unwrap_or_default(),
                    code
                ),
                title: None,
                description: None,
                created_at: String::new(),
                updated_at: String::new(),
                status: "active".to_string(),
                tags: vec![],
                expires_at: None,
                password_hash: None,
                max_access_count: None,
                access_count: 0,
                expired: false,
                last_latency_ms: None,
                last_status: None,
            })
        });
        match rows {
            Ok(r) => r.filter_map(|x| x.ok()).collect(),
            Err(_) => vec![],
        }
    };

    if urls.is_empty() {
        return Redirect::to("/admin/settings?error=No shortened URLs found to export")
            .into_response();
    }

    let proto = if state.config.cookie_secure {
        "https"
    } else {
        "http"
    };
    let host_header = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost:8654");
    let base_url = state
        .config
        .base_url
        .clone()
        .unwrap_or_else(|| format!("{}://{}", proto, host_header));

    match crate::services::bulk::export_qr_zip(&urls, &form.format, &base_url) {
        Ok(zip_data) => {
            // Write Audit Log
            let _ = write_audit_log(
                &state.admin_db.lock().unwrap(),
                &state,
                &user.username,
                "BULK_QR_EXPORT",
                Some("bulk"),
                Some("qr"),
                Some(&ip),
                headers.get("user-agent").and_then(|h| h.to_str().ok()),
            );

            Response::builder()
                .header("content-type", "application/zip")
                .header(
                    "content-disposition",
                    "attachment; filename=\"qr_codes.zip\"",
                )
                .body(axum::body::Body::from(zip_data))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(e) => {
            Redirect::to(&format!("/admin/settings?error=Export failed: {}", e)).into_response()
        }
    }
}

// GET /admin/status
pub async fn status_get(State(state): State<AppState>, jar: CookieJar) -> Response {
    let (user, _) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let app_status = "Healthy";

    let db_status = {
        let conn_ok = {
            let conn = state.admin_db.lock().unwrap();
            get_user_count(&conn).is_ok()
        };
        if conn_ok {
            format!(
                "Operational\n\nDatabase Files:\n{}",
                get_db_file_info(&state.config.data_dir)
            )
        } else {
            "Degraded (Database connections failed)".to_string()
        }
    };

    let queue_size = 0;
    let memory_usage = get_memory_usage();

    let uptime_duration = state.start_time.elapsed();
    let uptime = crate::utils::format_duration(uptime_duration);

    let urls = vec![];

    let template = crate::templates::StatusTemplate {
        admin_username: user.username,
        app_status,
        db_status,
        queue_size,
        memory_usage,
        uptime,
        version: crate::build_info::APP_VERSION,
        git_commit: crate::build_info::GIT_COMMIT,
        urls,
    };

    template.into_response()
}

// GET /user/status
pub async fn user_status_get(State(state): State<AppState>, jar: CookieJar) -> Response {
    let (user, _) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let user_dbs = match state.get_user_dbs(user.id) {
        Ok(dbs) => dbs,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    };

    let app_status = "Healthy";

    let db_status = {
        let conn_ok = {
            let conn = user_dbs.content.lock().unwrap();
            get_url_counts(&conn).is_ok()
        };
        if conn_ok {
            "Operational".to_string()
        } else {
            "Degraded (Database connection failed)".to_string()
        }
    };

    let queue_size = 0;
    let memory_usage = get_memory_usage();

    let uptime_duration = state.start_time.elapsed();
    let uptime = crate::utils::format_duration(uptime_duration);

    let urls = {
        let conn = user_dbs.content.lock().unwrap();
        crate::db::content::list_urls(&conn, 50, 0, None).unwrap_or_default()
    };

    let template = crate::templates::UserStatusTemplate {
        admin_username: user.username,
        app_status,
        db_status,
        queue_size,
        memory_usage,
        uptime,
        version: crate::build_info::APP_VERSION,
        git_commit: crate::build_info::GIT_COMMIT,
        urls,
    };

    template.into_response()
}

// POST /admin/settings/restore
pub async fn restore_backup_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    mut multipart: axum::extract::Multipart,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let ip = get_client_ip(&headers, connect_info);
    let mut file_bytes = Vec::new();
    let mut confirm_text = String::new();
    let mut csrf_token = String::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "backup_file" {
            if let Ok(bytes) = field.bytes().await {
                file_bytes = bytes.to_vec();
            }
        } else if name == "confirm_text" {
            if let Ok(text) = field.text().await {
                confirm_text = text.trim().to_string();
            }
        } else if name == "csrf_token" {
            if let Ok(token) = field.text().await {
                csrf_token = token.trim().to_string();
            }
        }
    }

    if !verify_csrf(&session_id, &csrf_token) {
        return Redirect::to("/admin/settings?error=Invalid CSRF token").into_response();
    }

    if confirm_text != "RESTORE" {
        return Redirect::to("/admin/settings?error=Confirmation text must be exactly 'RESTORE'")
            .into_response();
    }

    if file_bytes.is_empty() {
        return Redirect::to("/admin/settings?error=No backup file uploaded").into_response();
    }

    // Save uploaded archive to a temporary file
    let temp_file_path =
        std::env::temp_dir().join(format!("bzod_restore_{}.tar.gz", uuid::Uuid::new_v4()));
    if let Err(e) = std::fs::write(&temp_file_path, &file_bytes) {
        return Redirect::to(&format!(
            "/admin/settings?error=Failed to write temp file: {}",
            e
        ))
        .into_response();
    }

    // Log RESTORE_INITIATED audit event before restore
    {
        let conn = state.admin_db.lock().unwrap();
        let _ = write_audit_log(
            &conn,
            &state,
            &user.username,
            "RESTORE_INITIATED",
            Some("system"),
            Some("tarball"),
            Some(&ip),
            headers.get("user-agent").and_then(|h| h.to_str().ok()),
        );
    }

    // Call the perform_restore engine inside closed connection blocks
    let restore_res = {
        // Temporarily suspend access to active SQLite connections
        let mut admin_conn = state.admin_db.lock().unwrap();
        let mut system_conn = state.system_db.lock().unwrap();
        let mut users_conn = state.users_db.lock().unwrap();
        let mut urls_conn = state.db.global_urls.lock().unwrap();
        let mut pages_conn = state.db.global_landing_pages.lock().unwrap();
        let mut reserved_conn = state.db.reserved.lock().unwrap();

        // 1. Close current connections by replacing them with dummy in-memory DBs
        *admin_conn = rusqlite::Connection::open_in_memory()
            .unwrap_or_else(|_| rusqlite::Connection::open(":memory:").unwrap());
        *system_conn = rusqlite::Connection::open_in_memory()
            .unwrap_or_else(|_| rusqlite::Connection::open(":memory:").unwrap());
        *users_conn = rusqlite::Connection::open_in_memory()
            .unwrap_or_else(|_| rusqlite::Connection::open(":memory:").unwrap());
        *urls_conn = rusqlite::Connection::open_in_memory()
            .unwrap_or_else(|_| rusqlite::Connection::open(":memory:").unwrap());
        *pages_conn = rusqlite::Connection::open_in_memory()
            .unwrap_or_else(|_| rusqlite::Connection::open(":memory:").unwrap());
        *reserved_conn = rusqlite::Connection::open_in_memory()
            .unwrap_or_else(|_| rusqlite::Connection::open(":memory:").unwrap());

        // 2. Clear tenant pool cache
        if let Ok(mut pool) = state.user_dbs.lock() {
            pool.clear();
        }

        // 3. Perform restore unpacking/validation
        let res = crate::cli::restore::perform_restore(&temp_file_path, &state.config.data_dir);

        // 4. Reinitialize Core database connections
        let topology = crate::db::topology::Topology::new(&state.config.data_dir);
        let new_admin = rusqlite::Connection::open(topology.admin_db());
        let new_system = rusqlite::Connection::open(topology.system_db());
        let new_users = rusqlite::Connection::open(topology.users_registry_db());
        let new_urls = rusqlite::Connection::open(topology.global_urls_db());
        let new_pages = rusqlite::Connection::open(topology.global_landing_pages_db());
        let new_reserved = rusqlite::Connection::open(topology.reserved_db());

        match (
            new_admin,
            new_system,
            new_users,
            new_urls,
            new_pages,
            new_reserved,
        ) {
            (Ok(adm), Ok(sys), Ok(usr), Ok(urls), Ok(pages), Ok(resv)) => {
                let _ = crate::db::sqlite::enable_wal(&adm, "admin");
                let _ = crate::db::sqlite::enable_wal(&sys, "system");
                let _ = crate::db::sqlite::enable_wal(&usr, "users");
                let _ = crate::db::sqlite::enable_wal(&urls, "global_urls");
                let _ = crate::db::sqlite::enable_wal(&pages, "global_landing_pages");
                let _ = crate::db::sqlite::enable_wal(&resv, "reserved");

                let _ = crate::db::sqlite::enable_foreign_keys(&adm, "admin");
                let _ = crate::db::sqlite::enable_foreign_keys(&sys, "system");
                let _ = crate::db::sqlite::enable_foreign_keys(&usr, "users");
                let _ = crate::db::sqlite::enable_foreign_keys(&urls, "global_urls");
                let _ = crate::db::sqlite::enable_foreign_keys(&pages, "global_landing_pages");
                let _ = crate::db::sqlite::enable_foreign_keys(&resv, "reserved");

                *admin_conn = adm;
                *system_conn = sys;
                *users_conn = usr;
                *urls_conn = urls;
                *pages_conn = pages;
                *reserved_conn = resv;
            }
            _ => {
                return Redirect::to("/admin/settings?error=Failed to reopen restored databases")
                    .into_response();
            }
        }

        res
    };

    let _ = std::fs::remove_file(&temp_file_path);

    match restore_res {
        Ok(_) => {
            // Write database restore success log to newly restored admin db
            {
                let conn = state.admin_db.lock().unwrap();
                let _ = write_audit_log(
                    &conn,
                    &state,
                    &user.username,
                    "DATABASE_RESTORE",
                    Some("system"),
                    Some("tarball"),
                    Some(&ip),
                    headers.get("user-agent").and_then(|h| h.to_str().ok()),
                );
            }

            // Run doctor check to verify integrity after restore (spawn in background since we can't easily await here)
            tracing::info!("Running post-restore diagnostics...");
            let config_clone = state.config.clone();
            tokio::spawn(async move {
                let _ = crate::cli::doctor::run(None, config_clone).await;
            });

            Redirect::to("/admin/login").into_response()
        }
        Err(e) => {
            Redirect::to(&format!("/admin/settings?error=Restore failed: {}", e)).into_response()
        }
    }
}
