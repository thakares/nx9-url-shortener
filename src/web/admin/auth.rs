use super::*;

// GET /admin
pub async fn admin_index(State(state): State<AppState>, jar: CookieJar) -> Response {
    match require_auth(&state, &jar).await {
        Ok(_) => Redirect::to("/admin/dashboard").into_response(),
        Err(redir) => redir.into_response(),
    }
}

// GET /admin/login
pub async fn login_get(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let error = params.get("error").cloned();
    let csrf_token = generate_token(16);

    let secure_flag = crate::utils::resolve_cookie_secure(state.config.cookie_secure, &headers);
    let cookie = Cookie::build(("bzod_temp_csrf", csrf_token.clone()))
        .path("/admin/login")
        .secure(secure_flag)
        .http_only(true)
        .same_site(axum_extra::extract::cookie::SameSite::Strict)
        .max_age(time::Duration::minutes(10))
        .build();

    let new_jar = jar.add(cookie);
    let template = crate::templates::LoginTemplate {
        error,
        csrf_token,
        action: "/admin/login".to_string(),
        title: "Admin Login".to_string(),
        subtitle: "Administrative Access".to_string(),
        button_text: "Sign In".to_string(),
    };
    (new_jar, template).into_response()
}

#[derive(Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
    pub csrf_token: String,
}

/// Bootstrap is allowed only when the system has no real admin yet.
pub(crate) fn is_bootstrap_allowed(
    user_count: i64,
    admin_count: i64,
    active_session_count: i64,
) -> bool {
    user_count <= 1 && admin_count == 0 && active_session_count == 0
}

fn count_login_bootstrap_state(
    conn: &rusqlite::Connection,
) -> Result<(i64, i64, i64), rusqlite::Error> {
    let u_count: i64 = conn.query_row("SELECT COUNT(*) FROM users;", [], |r| r.get(0))?;
    let a_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM users WHERE account_type = 'admin';",
        [],
        |r| r.get(0),
    )?;
    let now = Utc::now().to_rfc3339();
    let s_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sessions WHERE expires_at > ?1;",
        [now],
        |r| r.get(0),
    )?;
    Ok((u_count, a_count, s_count))
}

/// Verify an existing admin tenant user may log into the admin UI.
///
/// Does not log the password. Rejection reasons are structured for observability.
pub(crate) fn verify_admin_credentials(
    user: &crate::models::TenantUser,
    password: &str,
) -> Result<(), &'static str> {
    if user.status != "active" {
        return Err("account_disabled");
    }
    if user.account_type != "admin" {
        return Err("insufficient_privileges");
    }
    if !verify_password(password, &user.password_hash) {
        return Err("invalid_credentials");
    }
    Ok(())
}

fn load_tenant_user_by_username(
    conn: &rusqlite::Connection,
    username: &str,
) -> Result<Option<crate::models::TenantUser>, rusqlite::Error> {
    crate::db::users::get_user_by_username(conn, username)
}

fn tenant_user_to_admin_user(u: crate::models::TenantUser) -> User {
    User {
        id: u.id.to_string(),
        username: u.username,
        password_hash: u.password_hash,
        created_at: u.created_at,
    }
}

fn audit_meta(ip: &str, headers: &HeaderMap) -> String {
    format!(
        "IP: {:?}, UA: {:?}",
        ip,
        headers.get("user-agent").and_then(|h| h.to_str().ok())
    )
}

// POST /admin/login
pub async fn login_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Form(form): Form<LoginForm>,
) -> Response {
    let temp_csrf = jar
        .get("bzod_temp_csrf")
        .map(|c| c.value().to_string())
        .unwrap_or_default();
    if temp_csrf.is_empty() || temp_csrf != form.csrf_token {
        return Redirect::to("/admin/login?error=Invalid CSRF token").into_response();
    }

    let ip = get_client_ip(&headers, connect_info);

    let bootstrap_allowed = {
        let conn = match state.users_db.lock() {
            Ok(c) => c,
            Err(_) => {
                return Redirect::to("/admin/login?error=Internal error").into_response();
            }
        };
        match count_login_bootstrap_state(&conn) {
            Ok((u, a, s)) => is_bootstrap_allowed(u, a, s),
            Err(e) => {
                tracing::error!(error = %e, "login bootstrap state query failed");
                false
            }
        }
    };

    let user_opt = if bootstrap_allowed
        && form.username == state.config.admin_username
        && verify_sha256(&form.password, &state.config.bootstrap_password_sha256)
    {
        // Bootstrap Phase using BOOTSTRAP_PASSWORD_SHA256
        let hash = match hash_password(&form.password) {
            Ok(h) => h,
            Err(_) => {
                return Redirect::to("/admin/login?error=Internal hashing error").into_response()
            }
        };

        let created_user_res = {
            let conn = match state.users_db.lock() {
                Ok(c) => c,
                Err(_) => {
                    return Redirect::to("/admin/login?error=Internal error").into_response();
                }
            };
            crate::db::users::create_admin_user(&conn, &form.username, &hash)
        };

        match created_user_res {
            Ok(u) => {
                if let Ok(system_conn) = state.system_db.lock() {
                    let metadata = audit_meta(&ip, &headers);
                    let _ = crate::db::audit_events::write_audit_event(
                        &system_conn,
                        &u.username,
                        "BOOTSTRAP_USER_PROVISIONED",
                        "user",
                        &u.id.to_string(),
                        Some(&metadata),
                    );
                }

                Some(User {
                    id: u.id.to_string(),
                    username: u.username,
                    password_hash: u.password_hash,
                    created_at: u.created_at,
                })
            }
            Err(e) => {
                tracing::error!("Failed to create admin user during bootstrap: {:?}", e);
                None
            }
        }
    } else {
        let conn = match state.users_db.lock() {
            Ok(c) => c,
            Err(_) => {
                return Redirect::to("/admin/login?error=Internal error").into_response();
            }
        };
        match load_tenant_user_by_username(&conn, &form.username) {
            Ok(Some(u)) => match verify_admin_credentials(&u, &form.password) {
                Ok(()) => Some(tenant_user_to_admin_user(u)),
                Err(reason) => {
                    tracing::warn!(username = form.username, reason, "login rejected");
                    None
                }
            },
            Ok(None) => {
                tracing::warn!(
                    username = form.username,
                    reason = "user_not_found",
                    "login rejected"
                );
                None
            }
            Err(e) => {
                tracing::error!(error = %e, "login user lookup failed");
                None
            }
        }
    };

    match user_opt {
        Some(user) => {
            let session_token = generate_token(32);
            let expires = (Utc::now() + chrono::Duration::days(30)).to_rfc3339();

            {
                let conn = match state.users_db.lock() {
                    Ok(c) => c,
                    Err(_) => {
                        return Redirect::to("/admin/login?error=Internal error").into_response();
                    }
                };
                let user_id_i64 = user.id.parse::<i64>().unwrap_or(0);
                let now = Utc::now().to_rfc3339();
                if let Err(e) = conn.execute(
                    "INSERT INTO sessions (id, user_id, expires_at, created_at) VALUES (?1, ?2, ?3, ?4);",
                    rusqlite::params![session_token, user_id_i64, expires, now],
                ) {
                    tracing::error!(error = %e, "failed to insert admin session");
                    return Redirect::to("/admin/login?error=Internal error").into_response();
                }

                if let Ok(system_conn) = state.system_db.lock() {
                    let metadata = audit_meta(&ip, &headers);
                    let _ = crate::db::audit_events::write_audit_event(
                        &system_conn,
                        &user.username,
                        "USER_LOGIN",
                        "session",
                        &session_token,
                        Some(&metadata),
                    );
                }
            }

            let secure_flag =
                crate::utils::resolve_cookie_secure(state.config.cookie_secure, &headers);
            let cookie = Cookie::build(("bzod_session", session_token))
                .path("/")
                .secure(secure_flag)
                .http_only(true)
                .same_site(axum_extra::extract::cookie::SameSite::Strict)
                .max_age(time::Duration::days(30))
                .build();

            let clear_temp = Cookie::build("bzod_temp_csrf")
                .path("/admin/login")
                .max_age(time::Duration::ZERO)
                .build();

            let mut response_jar = jar.clone();
            response_jar = response_jar.add(cookie).add(clear_temp);

            (response_jar, Redirect::to("/admin/dashboard")).into_response()
        }
        None => {
            if let Ok(system_conn) = state.system_db.lock() {
                let metadata = audit_meta(&ip, &headers);
                let _ = crate::db::audit_events::write_audit_event(
                    &system_conn,
                    "anonymous",
                    "LOGIN_FAILED",
                    "login",
                    "",
                    Some(&metadata),
                );
            }
            Redirect::to("/admin/login?error=Invalid username or password").into_response()
        }
    }
}

// GET /logout
pub async fn public_logout(State(_state): State<AppState>, jar: CookieJar) -> Response {
    let cookie = Cookie::build("bzod_user_session")
        .path("/")
        .max_age(time::Duration::ZERO)
        .build();

    let mut response_jar = jar.clone();
    response_jar = response_jar.add(cookie);

    (response_jar, Redirect::to("/login")).into_response()
}

// GET /login
pub async fn public_login_get(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let error = params.get("error").cloned();
    let csrf_token = generate_token(16);

    let secure_flag = crate::utils::resolve_cookie_secure(state.config.cookie_secure, &headers);
    let cookie = Cookie::build(("bzod_temp_csrf", csrf_token.clone()))
        .path("/login")
        .secure(secure_flag)
        .http_only(true)
        .same_site(axum_extra::extract::cookie::SameSite::Strict)
        .max_age(time::Duration::minutes(10))
        .build();

    let new_jar = jar.add(cookie);
    let template = crate::templates::LoginTemplate {
        error,
        csrf_token,
        action: "/login".to_string(),
        title: "User Login".to_string(),
        subtitle: "Standard account access".to_string(),
        button_text: "Sign In".to_string(),
    };
    (new_jar, template).into_response()
}

// POST /login
pub async fn public_login_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Form(form): Form<LoginForm>,
) -> Response {
    let temp_csrf = jar
        .get("bzod_temp_csrf")
        .map(|c| c.value().to_string())
        .unwrap_or_default();
    if temp_csrf.is_empty() || temp_csrf != form.csrf_token {
        return Redirect::to("/login?error=Invalid CSRF token").into_response();
    }

    let ip = get_client_ip(&headers, connect_info);

    let user_opt: Option<crate::models::TenantUser> = {
        let conn = state.users_db.lock().unwrap();
        match crate::db::users::get_user_by_username(&conn, &form.username) {
            Ok(Some(u)) => {
                if u.status != "active" {
                    None
                } else if verify_password(&form.password, &u.password_hash) {
                    Some(u)
                } else {
                    None
                }
            }
            _ => None,
        }
    };

    match user_opt {
        Some(user) => {
            let session_token = generate_token(32);
            let expires = (Utc::now() + chrono::Duration::days(30)).to_rfc3339();

            {
                let conn = state.users_db.lock().unwrap();
                let _ =
                    crate::db::users::create_user_session(&conn, &session_token, user.id, &expires);

                let system_conn = state.system_db.lock().unwrap();
                let metadata = format!(
                    "IP: {:?}, UA: {:?}",
                    ip,
                    headers.get("user-agent").and_then(|h| h.to_str().ok())
                );
                let _ = crate::db::audit_events::write_audit_event(
                    &system_conn,
                    &user.username,
                    "USER_LOGIN",
                    "session",
                    &session_token,
                    Some(&metadata),
                );
            }

            let secure_flag =
                crate::utils::resolve_cookie_secure(state.config.cookie_secure, &headers);
            let cookie = Cookie::build(("bzod_user_session", session_token))
                .path("/")
                .secure(secure_flag)
                .http_only(true)
                .same_site(axum_extra::extract::cookie::SameSite::Strict)
                .max_age(time::Duration::days(30))
                .build();

            let clear_temp = Cookie::build("bzod_temp_csrf")
                .path("/login")
                .max_age(time::Duration::ZERO)
                .build();

            let mut response_jar = jar.clone();
            response_jar = response_jar.add(cookie).add(clear_temp);

            (response_jar, Redirect::to("/user/dashboard")).into_response()
        }
        None => {
            let system_conn = state.system_db.lock().unwrap();
            let metadata = format!(
                "IP: {:?}, UA: {:?}",
                ip,
                headers.get("user-agent").and_then(|h| h.to_str().ok())
            );
            let _ = crate::db::audit_events::write_audit_event(
                &system_conn,
                "anonymous",
                "LOGIN_FAILED",
                "login",
                "",
                Some(&metadata),
            );
            Redirect::to("/login?error=Invalid username or password").into_response()
        }
    }
}

// GET /admin/logout
pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> Response {
    if let Ok((_, session_id)) = require_auth(&state, &jar).await {
        let conn = state.users_db.lock().unwrap();
        let _ = conn.execute("DELETE FROM sessions WHERE id = ?1;", [&session_id]);
    }

    let cookie = Cookie::build("bzod_session")
        .path("/")
        .max_age(time::Duration::ZERO)
        .build();

    let mut response_jar = jar.clone();
    response_jar = response_jar.add(cookie);

    (response_jar, Redirect::to("/admin/login")).into_response()
}

#[cfg(test)]
mod login_helpers_tests {
    use super::is_bootstrap_allowed;

    #[test]
    fn bootstrap_only_when_no_admin() {
        assert!(is_bootstrap_allowed(0, 0, 0));
        assert!(is_bootstrap_allowed(1, 0, 0));
        assert!(!is_bootstrap_allowed(2, 0, 0));
        assert!(!is_bootstrap_allowed(1, 1, 0));
        assert!(!is_bootstrap_allowed(1, 0, 1));
    }
}
