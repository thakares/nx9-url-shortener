use super::*;

// GET /user/dashboard
pub async fn user_dashboard_get(State(state): State<AppState>, jar: CookieJar) -> Response {
    let (user, _session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let user_dbs = match state.get_user_dbs(user.id) {
        Ok(dbs) => dbs,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    };

    let (total_urls, active_links, dead_links) = {
        let conn = user_dbs.content.lock().unwrap();
        get_url_counts(&conn).unwrap_or((0, 0, 0))
    };

    let total_pages = {
        let conn = user_dbs.content.lock().unwrap();
        get_landing_page_count(&conn).unwrap_or(0)
    };

    let total_clicks = {
        let conn = user_dbs.analytics.lock().unwrap();
        get_total_clicks(&conn).unwrap_or(0)
    };

    let clicks_data = {
        let conn = user_dbs.analytics.lock().unwrap();
        get_clicks_trend(&conn, "url", "all", 30)
            .or_else(|_| get_clicks_trend_raw(&conn, "url", "all", 30))
            .unwrap_or_default()
    };

    let mut trend_map = std::collections::BTreeMap::new();
    for i in (0..30).rev() {
        let date_str = (Utc::now() - chrono::Duration::days(i))
            .format("%Y-%m-%d")
            .to_string();
        trend_map.insert(date_str, 0i64);
    }
    for (d, c) in clicks_data {
        trend_map.insert(d, c);
    }
    let formatted_trend: Vec<(String, i64)> = trend_map.into_iter().collect();
    let traffic_chart = generate_line_chart(&formatted_trend);

    let countries_data = {
        let conn = user_dbs.analytics.lock().unwrap();
        get_metric_rankings(&conn, "url", "all", "country", 5)
            .or_else(|_| get_metric_rankings_raw(&conn, "url", "all", "country", 5))
            .unwrap_or_default()
    };
    let countries_chart = generate_bar_chart(&countries_data);

    let referrers_data = {
        let conn = user_dbs.analytics.lock().unwrap();
        get_metric_rankings(&conn, "url", "all", "referrer", 5)
            .or_else(|_| get_metric_rankings_raw(&conn, "url", "all", "referrer", 5))
            .unwrap_or_default()
    };
    let referrers_chart = generate_bar_chart(&referrers_data);

    let browsers_data = {
        let conn = user_dbs.analytics.lock().unwrap();
        get_metric_rankings(&conn, "url", "all", "browser", 5)
            .or_else(|_| get_metric_rankings_raw(&conn, "url", "all", "browser", 5))
            .unwrap_or_default()
    };
    let browsers_chart = generate_bar_chart(&browsers_data);

    let template = crate::templates::UserDashboardTemplate {
        admin_username: user.username,
        total_urls,
        total_pages,
        total_clicks,
        active_links,
        dead_links,
        traffic_chart,
        countries_chart,
        browsers_chart,
        referrers_chart,
    };

    template.into_response()
}

// GET /admin/dashboard
pub async fn dashboard_get(State(state): State<AppState>, jar: CookieJar) -> Response {
    let (user, _) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let (total_urls, active_links, dead_links) = {
        let conn = state.db.global_urls.lock().unwrap();
        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM global_urls WHERE status != 'retired';",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let active: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM global_urls WHERE status = 'active';",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let dead: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM global_urls WHERE status = 'disabled';",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        (total, active, dead)
    };

    let total_pages = {
        let conn = state.db.global_landing_pages.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM global_landing_pages WHERE status != 'retired';",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0)
    };

    let total_clicks = {
        let users_conn = state.users_db.lock().unwrap();
        crate::db::users::get_platform_total_clicks(&state.db.topology, &users_conn).unwrap_or(0)
    };

    let mut trend_map = std::collections::BTreeMap::new();
    for i in (0..30).rev() {
        let date_str = (Utc::now() - chrono::Duration::days(i))
            .format("%Y-%m-%d")
            .to_string();
        trend_map.insert(date_str, 0i64);
    }
    let formatted_trend: Vec<(String, i64)> = trend_map.into_iter().collect();
    let traffic_chart = generate_line_chart(&formatted_trend);

    let countries_chart = generate_bar_chart(&[]);
    let referrers_chart = generate_bar_chart(&[]);
    let browsers_chart = generate_bar_chart(&[]);

    let template = crate::templates::DashboardTemplate {
        admin_username: user.username,
        total_urls,
        total_pages,
        total_clicks,
        active_links,
        dead_links,
        traffic_chart,
        countries_chart,
        browsers_chart,
        referrers_chart,
    };

    template.into_response()
}
