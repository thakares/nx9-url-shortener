use super::*;

// GET /admin/audit
pub async fn audit_get(State(state): State<AppState>, jar: CookieJar) -> Response {
    let (user, _) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let logs = {
        let conn = state.system_db.lock().unwrap();
        let events = crate::db::audit_events::list_audit_events(&conn, 100, 0, None, None)
            .unwrap_or_default();
        events
            .into_iter()
            .map(|e| {
                let (ip, ua) = if let Some(ref m) = e.metadata {
                    if m.starts_with("IP: ") {
                        let parts: Vec<&str> = m.split(", UA: ").collect();
                        let ip = parts[0]
                            .trim_start_matches("IP: ")
                            .trim_matches('"')
                            .trim_matches('\'')
                            .replace("Some(", "")
                            .replace(")", "");
                        let ua = if parts.len() > 1 {
                            parts[1]
                                .trim_matches('"')
                                .trim_matches('\'')
                                .replace("Some(", "")
                                .replace(")", "")
                        } else {
                            "Unknown".to_string()
                        };
                        (Some(ip), Some(ua))
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                };

                crate::models::AuditLog {
                    id: e.id,
                    timestamp: e.timestamp,
                    username: e.actor,
                    action: e.action,
                    object_type: Some(e.object_type),
                    object_id: Some(e.object_id),
                    ip_address: ip,
                    user_agent: ua,
                }
            })
            .collect()
    };

    let template = crate::templates::AuditTemplate {
        admin_username: user.username,
        logs,
    };

    template.into_response()
}

// GET /user/audit
pub async fn user_audit_get(State(state): State<AppState>, jar: CookieJar) -> Response {
    let (user, _) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let logs = {
        let conn = state.system_db.lock().unwrap();
        let events =
            crate::db::audit_events::list_audit_events(&conn, 100, 0, Some(&user.username), None)
                .unwrap_or_default();
        events
            .into_iter()
            .map(|e| {
                let (ip, ua) = if let Some(ref m) = e.metadata {
                    if m.starts_with("IP: ") {
                        let parts: Vec<&str> = m.split(", UA: ").collect();
                        let ip = parts[0]
                            .trim_start_matches("IP: ")
                            .trim_matches('"')
                            .trim_matches('\'')
                            .replace("Some(", "")
                            .replace(")", "");
                        let ua = if parts.len() > 1 {
                            parts[1]
                                .trim_matches('"')
                                .trim_matches('\'')
                                .replace("Some(", "")
                                .replace(")", "")
                        } else {
                            "Unknown".to_string()
                        };
                        (Some(ip), Some(ua))
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                };

                crate::models::AuditLog {
                    id: e.id,
                    timestamp: e.timestamp,
                    username: e.actor,
                    action: e.action,
                    object_type: Some(e.object_type),
                    object_id: Some(e.object_id),
                    ip_address: ip,
                    user_agent: ua,
                }
            })
            .collect()
    };

    let template = crate::templates::UserAuditTemplate {
        admin_username: user.username,
        logs,
    };

    template.into_response()
}
