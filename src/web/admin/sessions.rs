use super::*;

// GET /admin/sessions
pub async fn sessions_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let sessions = {
        let conn = state.users_db.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, user_id, expires_at, created_at FROM sessions ORDER BY created_at DESC;").unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok(crate::models::UserSession {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    expires_at: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    };

    let csrf_token = generate_csrf_token(&session_id);

    let template = crate::templates::SessionsTemplate {
        admin_username: user.username,
        sessions,
        csrf_token,
        success: params.get("success").cloned(),
        error: params.get("error").cloned(),
    };

    template.into_response()
}

// POST /admin/sessions/revoke/:id
pub async fn sessions_revoke_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let form_csrf = form.get("csrf_token").cloned().unwrap_or_default();
    if !verify_csrf(&session_id, &form_csrf) {
        return Redirect::to("/admin/sessions?error=Invalid CSRF token").into_response();
    }

    {
        let conn = state.users_db.lock().unwrap();
        let _ = conn.execute("DELETE FROM sessions WHERE id = ?1;", [&id]);
    }

    {
        let conn_sys = state.system_db.lock().unwrap();
        let _ = crate::db::audit_events::write_audit_event(
            &conn_sys,
            &user.username,
            "SESSION_REVOKED",
            "session",
            &id,
            None,
        );
    }

    Redirect::to("/admin/sessions?success=Session revoked").into_response()
}

// POST /admin/sessions/revoke-all
pub async fn sessions_revoke_all_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let form_csrf = form.get("csrf_token").cloned().unwrap_or_default();
    if !verify_csrf(&session_id, &form_csrf) {
        return Redirect::to("/admin/sessions?error=Invalid CSRF token").into_response();
    }

    {
        let conn = state.users_db.lock().unwrap();
        let _ = conn.execute("DELETE FROM sessions;", []);
    }

    {
        let conn_sys = state.system_db.lock().unwrap();
        let _ = crate::db::audit_events::write_audit_event(
            &conn_sys,
            &user.username,
            "SESSIONS_ALL_REVOKED",
            "session",
            "all",
            None,
        );
    }

    Redirect::to("/admin/sessions?success=All active sessions revoked").into_response()
}
