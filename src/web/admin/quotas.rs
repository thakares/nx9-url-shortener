use super::*;

// GET /admin/quotas
pub async fn quotas_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let quotas = {
        let conn = state.users_db.lock().unwrap();
        let mut stmt = conn.prepare("SELECT user_id, max_urls, max_landings, max_api_tokens, max_storage_mb, current_urls, current_landings, current_api_tokens, current_storage_mb FROM quotas ORDER BY user_id ASC;").unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok(crate::models::UserQuotas {
                    user_id: row.get(0)?,
                    max_urls: row.get(1)?,
                    max_landings: row.get(2)?,
                    max_api_tokens: row.get(3)?,
                    max_storage_mb: row.get(4)?,
                    current_urls: row.get(5)?,
                    current_landings: row.get(6)?,
                    current_api_tokens: row.get(7)?,
                    current_storage_mb: row.get(8)?,
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    };

    let csrf_token = generate_csrf_token(&session_id);

    let template = crate::templates::QuotasTemplate {
        admin_username: user.username,
        quotas,
        csrf_token,
        success: params.get("success").cloned(),
        error: params.get("error").cloned(),
    };

    template.into_response()
}

// POST /admin/quotas
pub async fn quotas_post(
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
        return Redirect::to("/admin/quotas?error=Invalid CSRF token").into_response();
    }

    let action = form.get("action").cloned().unwrap_or_default();
    let users_conn = state.users_db.lock().unwrap();

    if action == "reconcile_all" {
        let user_ids: Vec<i64> = {
            let mut stmt = users_conn.prepare("SELECT id FROM users;").unwrap();
            let rows = stmt.query_map([], |row| row.get(0)).unwrap();
            rows.filter_map(|r| r.ok()).collect()
        };

        for uid in user_ids {
            if let Ok(user_dbs) = state.get_user_dbs(uid) {
                let user_content_conn = user_dbs.content.lock().unwrap();
                let _ =
                    crate::db::users::reconcile_user_quotas(&users_conn, uid, &user_content_conn);
            }
        }

        let system_conn = state.system_db.lock().unwrap();
        let _ = crate::db::audit_events::write_audit_event(
            &system_conn,
            &user.username,
            "QUOTAS_RECONCILE_ALL",
            "quota",
            "all",
            None,
        );

        Redirect::to("/admin/quotas?success=All user quotas reconciled successfully")
            .into_response()
    } else if action == "reconcile" {
        let uid_str = form.get("user_id").cloned().unwrap_or_default();
        let uid = uid_str.parse::<i64>().unwrap_or(0);
        if let Ok(user_dbs) = state.get_user_dbs(uid) {
            let user_content_conn = user_dbs.content.lock().unwrap();
            let _ = crate::db::users::reconcile_user_quotas(&users_conn, uid, &user_content_conn);

            let system_conn = state.system_db.lock().unwrap();
            let _ = crate::db::audit_events::write_audit_event(
                &system_conn,
                &user.username,
                "QUOTAS_RECONCILE",
                "quota",
                &uid.to_string(),
                None,
            );
            Redirect::to(&format!("/admin/users/{}?success=Quotas reconciled", uid)).into_response()
        } else {
            Redirect::to("/admin/quotas?error=User databases not found").into_response()
        }
    } else {
        Redirect::to("/admin/quotas?error=Invalid action").into_response()
    }
}
