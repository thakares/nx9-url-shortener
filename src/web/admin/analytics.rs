use super::*;

// GET /admin/analytics/url/:id
pub async fn url_analytics_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(query): Query<AnalyticsQuery>,
) -> Response {
    let (user, _) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let url = {
        let conn = state.content_db.lock().unwrap();
        match get_url_by_id(&conn, &id) {
            Ok(Some(u)) => u,
            Ok(None) => return (StatusCode::NOT_FOUND, "URL not found").into_response(),
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
        }
    };

    let conn = state.analytics_db.lock().unwrap();

    let schema_cols = get_visits_schema_columns(&conn).unwrap_or_default();
    let has_utm_source = schema_cols.contains("utm_source");
    let has_utm_campaign = schema_cols.contains("utm_campaign");
    if has_utm_source || has_utm_campaign {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "UTM mapping verification failed",
        )
            .into_response();
    }

    let (clean_date_from, clean_date_to) =
        match validate_date_filters(query.date_from.as_deref(), query.date_to.as_deref()) {
            Ok(res) => res,
            Err(status) => return status.into_response(),
        };

    let total_clicks = get_target_visit_total_filtered(
        &conn,
        "url",
        &id,
        clean_date_from.as_deref(),
        clean_date_to.as_deref(),
    )
    .unwrap_or(0);

    let unique_visitors = get_target_unique_visitors(&conn, "url", &id).unwrap_or(0);
    let qr_scans = crate::db::qr::get_qr_scan_count(&conn, &id).unwrap_or(0);
    let direct_clicks = (total_clicks - qr_scans).max(0);

    let clicks_data = get_clicks_trend(&conn, "url", &id, 30)
        .or_else(|_| get_clicks_trend_raw(&conn, "url", &id, 30))
        .unwrap_or_default();
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

    let monthly_data = get_monthly_clicks_trend(&conn, "url", &id, 12).unwrap_or_default();
    let monthly_chart = generate_line_chart(&monthly_data);

    let countries_data = get_metric_rankings(&conn, "url", &id, "country", 5)
        .or_else(|_| get_metric_rankings_raw(&conn, "url", &id, "country", 5))
        .unwrap_or_default();
    let countries_chart = generate_bar_chart(&countries_data);

    let referrers_data = get_metric_rankings(&conn, "url", &id, "referrer", 5)
        .or_else(|_| get_metric_rankings_raw(&conn, "url", &id, "referrer", 5))
        .unwrap_or_default();
    let referrers_chart = generate_bar_chart(&referrers_data);

    let browsers_data = get_metric_rankings(&conn, "url", &id, "browser", 5)
        .or_else(|_| get_metric_rankings_raw(&conn, "url", &id, "browser", 5))
        .unwrap_or_default();
    let browsers_chart = generate_bar_chart(&browsers_data);

    let calculated_total_pages = (total_clicks as usize).div_ceil(ANALYTICS_PAGE_SIZE);
    let total_pages = std::cmp::max(1, calculated_total_pages);
    let requested_page = query.analytics_page.unwrap_or(1);
    let current_page = if requested_page == 0 {
        1
    } else {
        requested_page
    }
    .clamp(1, total_pages);
    let offset = (current_page - 1) * ANALYTICS_PAGE_SIZE;

    let visits_raw = get_target_visits_paginated(
        &conn,
        "url",
        &id,
        ANALYTICS_PAGE_SIZE as i64,
        offset as i64,
        clean_date_from.as_deref(),
        clean_date_to.as_deref(),
    )
    .unwrap_or_default();

    let visits: Vec<crate::templates::VisitorLogEntry> = visits_raw
        .into_iter()
        .enumerate()
        .map(|(idx, r)| {
            let (browser, _, _) = parse_ua(&r.user_agent);
            let referrer = clean_referrer(&r.referer);
            let sr = offset + idx + 1;
            crate::templates::VisitorLogEntry {
                sr,
                timestamp: r.timestamp,
                ip_address: r.ip_address,
                country: if r.country.is_empty() {
                    "Unknown".to_string()
                } else {
                    r.country
                },
                referrer,
                browser,
                user_agent: r.user_agent,
                utm_source: "-".to_string(),
                utm_campaign: "-".to_string(),
            }
        })
        .collect();

    let start_page = current_page.saturating_sub(3).max(1);
    let end_page = std::cmp::min(total_pages, current_page + 3);
    let visible_pages: Vec<usize> = (start_page..=end_page).collect();

    let page_start = if total_clicks == 0 { 0 } else { offset + 1 };
    let page_end = if total_clicks == 0 {
        0
    } else {
        std::cmp::min(total_clicks as usize, offset + ANALYTICS_PAGE_SIZE)
    };

    let template = crate::templates::UrlAnalyticsTemplate {
        admin_username: user.username,
        url,
        total_clicks,
        unique_visitors,
        qr_scans,
        direct_clicks,
        traffic_chart,
        monthly_chart,
        countries_chart,
        referrers_chart,
        browsers_chart,
        visits,
        current_page,
        total_pages,
        visible_pages,
        total_records: total_clicks,
        page_start,
        page_end,
        date_from: clean_date_from,
        date_to: clean_date_to,
        is_admin: true,
    };

    template.into_response()
}

// GET /admin/analytics/page/:id
pub async fn page_analytics_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(query): Query<AnalyticsQuery>,
) -> Response {
    let (user, _) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let page = {
        let conn = state.content_db.lock().unwrap();
        match get_landing_page_by_id(&conn, &id) {
            Ok(Some(p)) => p,
            Ok(None) => return (StatusCode::NOT_FOUND, "Landing page not found").into_response(),
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
        }
    };

    let conn = state.analytics_db.lock().unwrap();

    let schema_cols = get_visits_schema_columns(&conn).unwrap_or_default();
    let has_utm_source = schema_cols.contains("utm_source");
    let has_utm_campaign = schema_cols.contains("utm_campaign");
    if has_utm_source || has_utm_campaign {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "UTM mapping verification failed",
        )
            .into_response();
    }

    let (clean_date_from, clean_date_to) =
        match validate_date_filters(query.date_from.as_deref(), query.date_to.as_deref()) {
            Ok(res) => res,
            Err(status) => return status.into_response(),
        };

    let total_views = get_target_visit_total_filtered(
        &conn,
        "page",
        &id,
        clean_date_from.as_deref(),
        clean_date_to.as_deref(),
    )
    .unwrap_or(0);

    let unique_visitors = get_target_unique_visitors(&conn, "page", &id).unwrap_or(0);

    let clicks_data = get_clicks_trend(&conn, "page", &id, 30)
        .or_else(|_| get_clicks_trend_raw(&conn, "page", &id, 30))
        .unwrap_or_default();
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

    let monthly_data = get_monthly_clicks_trend(&conn, "page", &id, 12).unwrap_or_default();
    let monthly_chart = generate_line_chart(&monthly_data);

    let countries_data = get_metric_rankings(&conn, "page", &id, "country", 5)
        .or_else(|_| get_metric_rankings_raw(&conn, "page", &id, "country", 5))
        .unwrap_or_default();
    let countries_chart = generate_bar_chart(&countries_data);

    let referrers_data = get_metric_rankings(&conn, "page", &id, "referrer", 5)
        .or_else(|_| get_metric_rankings_raw(&conn, "page", &id, "referrer", 5))
        .unwrap_or_default();
    let referrers_chart = generate_bar_chart(&referrers_data);

    let calculated_total_pages = (total_views as usize).div_ceil(ANALYTICS_PAGE_SIZE);
    let total_pages = std::cmp::max(1, calculated_total_pages);
    let requested_page = query.analytics_page.unwrap_or(1);
    let current_page = if requested_page == 0 {
        1
    } else {
        requested_page
    }
    .clamp(1, total_pages);
    let offset = (current_page - 1) * ANALYTICS_PAGE_SIZE;

    let visits_raw = get_target_visits_paginated(
        &conn,
        "page",
        &id,
        ANALYTICS_PAGE_SIZE as i64,
        offset as i64,
        clean_date_from.as_deref(),
        clean_date_to.as_deref(),
    )
    .unwrap_or_default();

    let visits: Vec<crate::templates::VisitorLogEntry> = visits_raw
        .into_iter()
        .enumerate()
        .map(|(idx, r)| {
            let (browser, _, _) = parse_ua(&r.user_agent);
            let referrer = clean_referrer(&r.referer);
            let sr = offset + idx + 1;
            crate::templates::VisitorLogEntry {
                sr,
                timestamp: r.timestamp,
                ip_address: r.ip_address,
                country: if r.country.is_empty() {
                    "Unknown".to_string()
                } else {
                    r.country
                },
                referrer,
                browser,
                user_agent: r.user_agent,
                utm_source: "-".to_string(),
                utm_campaign: "-".to_string(),
            }
        })
        .collect();

    let start_page = current_page.saturating_sub(3).max(1);
    let end_page = std::cmp::min(total_pages, current_page + 3);
    let visible_pages: Vec<usize> = (start_page..=end_page).collect();

    let page_start = if total_views == 0 { 0 } else { offset + 1 };
    let page_end = if total_views == 0 {
        0
    } else {
        std::cmp::min(total_views as usize, offset + ANALYTICS_PAGE_SIZE)
    };

    let template = crate::templates::PageAnalyticsTemplate {
        admin_username: user.username,
        page,
        total_views,
        unique_visitors,
        traffic_chart,
        monthly_chart,
        countries_chart,
        referrers_chart,
        visits,
        current_page,
        total_pages,
        visible_pages,
        total_records: total_views,
        page_start,
        page_end,
        date_from: clean_date_from,
        date_to: clean_date_to,
        is_admin: true,
    };

    template.into_response()
}

pub async fn url_analytics_csv_export(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(query): Query<AnalyticsQuery>,
) -> Response {
    if require_auth(&state, &jar).await.is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    perform_csv_export(state, "url", id, query.date_from, query.date_to).await
}

pub async fn url_analytics_json_export(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(query): Query<AnalyticsQuery>,
) -> Response {
    if require_auth(&state, &jar).await.is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    perform_json_export(state, "url", id, query.date_from, query.date_to).await
}

pub async fn page_analytics_csv_export(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(query): Query<AnalyticsQuery>,
) -> Response {
    if require_auth(&state, &jar).await.is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    perform_csv_export(state, "page", id, query.date_from, query.date_to).await
}

pub async fn page_analytics_json_export(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(query): Query<AnalyticsQuery>,
) -> Response {
    if require_auth(&state, &jar).await.is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    perform_json_export(state, "page", id, query.date_from, query.date_to).await
}

// GET /analytics
pub async fn user_analytics_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let user_dbs = match state.get_user_dbs(user.id) {
        Ok(dbs) => dbs,
        Err(_) => return Redirect::to("/user/dashboard?error=Database error").into_response(),
    };

    let conn = user_dbs.analytics.lock().unwrap();

    let total_clicks = conn
        .query_row("SELECT COUNT(*) FROM visits;", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0);
    let unique_visitors = conn
        .query_row(
            "SELECT COUNT(DISTINCT ip_address) FROM visits;",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0);
    let direct_clicks = conn
        .query_row(
            "SELECT COUNT(*) FROM visits WHERE referer = '' OR referer = 'direct';",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0);
    let referred_clicks = total_clicks - direct_clicks;

    let referrers_chart = "<div>No referrer channels logged yet</div>".to_string();
    let browsers_chart = "<div>No browser traffic logged yet</div>".to_string();

    let visits = {
        let mut stmt = conn.prepare("SELECT id, target_type, target_id, timestamp, ip_address, user_agent, referer, accept_language, country, status_code FROM visits ORDER BY timestamp DESC LIMIT 50;").unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok(crate::models::VisitRecord {
                    id: row.get(0)?,
                    target_type: row.get(1)?,
                    target_id: row.get(2)?,
                    timestamp: row.get(3)?,
                    ip_address: row.get(4)?,
                    user_agent: row.get(5)?,
                    referer: row.get(6)?,
                    accept_language: row.get(7)?,
                    country: row.get(8)?,
                    status_code: row.get(9)?,
                    owner_user_id: Some(user.id),
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    };

    let csrf_token = generate_csrf_token(&session_id);

    let template = crate::templates::UserAnalyticsTemplate {
        admin_username: user.username.clone(),
        username: user.username,
        total_clicks,
        unique_visitors,
        direct_clicks,
        referred_clicks,
        referrers_chart,
        browsers_chart,
        visits,
        csrf_token,
        success: params.get("success").cloned(),
        error: params.get("error").cloned(),
    };

    template.into_response()
}

pub async fn user_url_analytics_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(query): Query<AnalyticsQuery>,
) -> Response {
    let (user, _session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    // Ownership check
    let (owner_user_id, target_type) = {
        let conn = state.system_db.lock().unwrap();
        match conn.query_row(
            "SELECT owner_user_id, target_type FROM global_slugs WHERE target_id = ?1",
            [&id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        ) {
            Ok(val) => val,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                return Redirect::to("/user/urls?error=Link not found").into_response();
            }
            Err(_) => return Redirect::to("/user/urls?error=Database error").into_response(),
        }
    };

    if owner_user_id != user.id || target_type != "url" {
        return StatusCode::FORBIDDEN.into_response();
    }

    let user_dbs = match state.get_user_dbs(user.id) {
        Ok(dbs) => dbs,
        Err(_) => return Redirect::to("/user/urls?error=Database error").into_response(),
    };

    let url = {
        let conn = user_dbs.content.lock().unwrap();
        match crate::db::content::get_url_by_id(&conn, &id) {
            Ok(Some(u)) => u,
            Ok(None) => return Redirect::to("/user/urls?error=Link not found").into_response(),
            Err(_) => return Redirect::to("/user/urls?error=Database error").into_response(),
        }
    };

    let conn = user_dbs.analytics.lock().unwrap();

    let schema_cols = get_visits_schema_columns(&conn).unwrap_or_default();
    let has_utm_source = schema_cols.contains("utm_source");
    let has_utm_campaign = schema_cols.contains("utm_campaign");
    if has_utm_source || has_utm_campaign {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "UTM mapping verification failed",
        )
            .into_response();
    }

    let (clean_date_from, clean_date_to) =
        match validate_date_filters(query.date_from.as_deref(), query.date_to.as_deref()) {
            Ok(res) => res,
            Err(status) => return status.into_response(),
        };

    let total_clicks = get_target_visit_total_filtered(
        &conn,
        "url",
        &id,
        clean_date_from.as_deref(),
        clean_date_to.as_deref(),
    )
    .unwrap_or(0);

    let unique_visitors = get_target_unique_visitors(&conn, "url", &id).unwrap_or(0);
    let qr_scans = crate::db::qr::get_qr_scan_count(&conn, &id).unwrap_or(0);
    let direct_clicks = (total_clicks - qr_scans).max(0);

    let clicks_data = get_clicks_trend(&conn, "url", &id, 30)
        .or_else(|_| get_clicks_trend_raw(&conn, "url", &id, 30))
        .unwrap_or_default();
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

    let monthly_data = get_monthly_clicks_trend(&conn, "url", &id, 12).unwrap_or_default();
    let monthly_chart = generate_line_chart(&monthly_data);

    let countries_data = get_metric_rankings(&conn, "url", &id, "country", 5)
        .or_else(|_| get_metric_rankings_raw(&conn, "url", &id, "country", 5))
        .unwrap_or_default();
    let countries_chart = generate_bar_chart(&countries_data);

    let referrers_data = get_metric_rankings(&conn, "url", &id, "referrer", 5)
        .or_else(|_| get_metric_rankings_raw(&conn, "url", &id, "referrer", 5))
        .unwrap_or_default();
    let referrers_chart = generate_bar_chart(&referrers_data);

    let browsers_data = get_metric_rankings(&conn, "url", &id, "browser", 5)
        .or_else(|_| get_metric_rankings_raw(&conn, "url", &id, "browser", 5))
        .unwrap_or_default();
    let browsers_chart = generate_bar_chart(&browsers_data);

    let calculated_total_pages = (total_clicks as usize).div_ceil(ANALYTICS_PAGE_SIZE);
    let total_pages = std::cmp::max(1, calculated_total_pages);
    let requested_page = query.analytics_page.unwrap_or(1);
    let current_page = if requested_page == 0 {
        1
    } else {
        requested_page
    }
    .clamp(1, total_pages);
    let offset = (current_page - 1) * ANALYTICS_PAGE_SIZE;

    let visits_raw = get_target_visits_paginated(
        &conn,
        "url",
        &id,
        ANALYTICS_PAGE_SIZE as i64,
        offset as i64,
        clean_date_from.as_deref(),
        clean_date_to.as_deref(),
    )
    .unwrap_or_default();

    let visits: Vec<crate::templates::VisitorLogEntry> = visits_raw
        .into_iter()
        .enumerate()
        .map(|(idx, r)| {
            let (browser, _, _) = parse_ua(&r.user_agent);
            let referrer = clean_referrer(&r.referer);
            let sr = offset + idx + 1;
            crate::templates::VisitorLogEntry {
                sr,
                timestamp: r.timestamp,
                ip_address: r.ip_address,
                country: if r.country.is_empty() {
                    "Unknown".to_string()
                } else {
                    r.country
                },
                referrer,
                browser,
                user_agent: r.user_agent,
                utm_source: "-".to_string(),
                utm_campaign: "-".to_string(),
            }
        })
        .collect();

    let start_page = current_page.saturating_sub(3).max(1);
    let end_page = std::cmp::min(total_pages, current_page + 3);
    let visible_pages: Vec<usize> = (start_page..=end_page).collect();

    let page_start = if total_clicks == 0 { 0 } else { offset + 1 };
    let page_end = if total_clicks == 0 {
        0
    } else {
        std::cmp::min(total_clicks as usize, offset + ANALYTICS_PAGE_SIZE)
    };

    let template = crate::templates::UrlAnalyticsTemplate {
        admin_username: user.username,
        url,
        total_clicks,
        unique_visitors,
        qr_scans,
        direct_clicks,
        traffic_chart,
        monthly_chart,
        countries_chart,
        referrers_chart,
        browsers_chart,
        visits,
        current_page,
        total_pages,
        visible_pages,
        total_records: total_clicks,
        page_start,
        page_end,
        date_from: clean_date_from,
        date_to: clean_date_to,
        is_admin: false,
    };

    template.into_response()
}

pub async fn user_page_analytics_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(query): Query<AnalyticsQuery>,
) -> Response {
    let (user, _session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    // Ownership check
    let (owner_user_id, target_type) = {
        let conn = state.system_db.lock().unwrap();
        match conn.query_row(
            "SELECT owner_user_id, target_type FROM global_slugs WHERE target_id = ?1",
            [&id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        ) {
            Ok(val) => val,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                return Redirect::to("/user/pages?error=Landing page not found").into_response();
            }
            Err(_) => return Redirect::to("/user/pages?error=Database error").into_response(),
        }
    };

    if owner_user_id != user.id || target_type != "page" {
        return StatusCode::FORBIDDEN.into_response();
    }

    let user_dbs = match state.get_user_dbs(user.id) {
        Ok(dbs) => dbs,
        Err(_) => return Redirect::to("/user/pages?error=Database error").into_response(),
    };

    let page = {
        let conn = user_dbs.content.lock().unwrap();
        match crate::db::content::get_landing_page_by_id(&conn, &id) {
            Ok(Some(p)) => p,
            Ok(None) => {
                return Redirect::to("/user/pages?error=Landing page not found").into_response()
            }
            Err(_) => return Redirect::to("/user/pages?error=Database error").into_response(),
        }
    };

    let conn = user_dbs.analytics.lock().unwrap();

    let schema_cols = get_visits_schema_columns(&conn).unwrap_or_default();
    let has_utm_source = schema_cols.contains("utm_source");
    let has_utm_campaign = schema_cols.contains("utm_campaign");
    if has_utm_source || has_utm_campaign {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "UTM mapping verification failed",
        )
            .into_response();
    }

    let (clean_date_from, clean_date_to) =
        match validate_date_filters(query.date_from.as_deref(), query.date_to.as_deref()) {
            Ok(res) => res,
            Err(status) => return status.into_response(),
        };

    let total_views = get_target_visit_total_filtered(
        &conn,
        "page",
        &id,
        clean_date_from.as_deref(),
        clean_date_to.as_deref(),
    )
    .unwrap_or(0);

    let unique_visitors = get_target_unique_visitors(&conn, "page", &id).unwrap_or(0);

    let clicks_data = get_clicks_trend(&conn, "page", &id, 30)
        .or_else(|_| get_clicks_trend_raw(&conn, "page", &id, 30))
        .unwrap_or_default();
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

    let monthly_data = get_monthly_clicks_trend(&conn, "page", &id, 12).unwrap_or_default();
    let monthly_chart = generate_line_chart(&monthly_data);

    let countries_data = get_metric_rankings(&conn, "page", &id, "country", 5)
        .or_else(|_| get_metric_rankings_raw(&conn, "page", &id, "country", 5))
        .unwrap_or_default();
    let countries_chart = generate_bar_chart(&countries_data);

    let referrers_data = get_metric_rankings(&conn, "page", &id, "referrer", 5)
        .or_else(|_| get_metric_rankings_raw(&conn, "page", &id, "referrer", 5))
        .unwrap_or_default();
    let referrers_chart = generate_bar_chart(&referrers_data);

    let calculated_total_pages = (total_views as usize).div_ceil(ANALYTICS_PAGE_SIZE);
    let total_pages = std::cmp::max(1, calculated_total_pages);
    let requested_page = query.analytics_page.unwrap_or(1);
    let current_page = if requested_page == 0 {
        1
    } else {
        requested_page
    }
    .clamp(1, total_pages);
    let offset = (current_page - 1) * ANALYTICS_PAGE_SIZE;

    let visits_raw = get_target_visits_paginated(
        &conn,
        "page",
        &id,
        ANALYTICS_PAGE_SIZE as i64,
        offset as i64,
        clean_date_from.as_deref(),
        clean_date_to.as_deref(),
    )
    .unwrap_or_default();

    let visits: Vec<crate::templates::VisitorLogEntry> = visits_raw
        .into_iter()
        .enumerate()
        .map(|(idx, r)| {
            let (browser, _, _) = parse_ua(&r.user_agent);
            let referrer = clean_referrer(&r.referer);
            let sr = offset + idx + 1;
            crate::templates::VisitorLogEntry {
                sr,
                timestamp: r.timestamp,
                ip_address: r.ip_address,
                country: if r.country.is_empty() {
                    "Unknown".to_string()
                } else {
                    r.country
                },
                referrer,
                browser,
                user_agent: r.user_agent,
                utm_source: "-".to_string(),
                utm_campaign: "-".to_string(),
            }
        })
        .collect();

    let start_page = current_page.saturating_sub(3).max(1);
    let end_page = std::cmp::min(total_pages, current_page + 3);
    let visible_pages: Vec<usize> = (start_page..=end_page).collect();

    let page_start = if total_views == 0 { 0 } else { offset + 1 };
    let page_end = if total_views == 0 {
        0
    } else {
        std::cmp::min(total_views as usize, offset + ANALYTICS_PAGE_SIZE)
    };

    let template = crate::templates::PageAnalyticsTemplate {
        admin_username: user.username,
        page,
        total_views,
        unique_visitors,
        traffic_chart,
        monthly_chart,
        countries_chart,
        referrers_chart,
        visits,
        current_page,
        total_pages,
        visible_pages,
        total_records: total_views,
        page_start,
        page_end,
        date_from: clean_date_from,
        date_to: clean_date_to,
        is_admin: false,
    };

    template.into_response()
}

pub async fn user_url_analytics_csv_export(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(query): Query<AnalyticsQuery>,
) -> Response {
    let (user, _session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    // Ownership check
    let (owner_user_id, target_type) = {
        let conn = state.system_db.lock().unwrap();
        match conn.query_row(
            "SELECT owner_user_id, target_type FROM global_slugs WHERE target_id = ?1",
            [&id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        ) {
            Ok(val) => val,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                return StatusCode::NOT_FOUND.into_response();
            }
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    };

    if owner_user_id != user.id || target_type != "url" {
        return StatusCode::FORBIDDEN.into_response();
    }

    let user_dbs = match state.get_user_dbs(user.id) {
        Ok(dbs) => dbs,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    perform_user_csv_export(user_dbs, "url", id, query.date_from, query.date_to).await
}

pub async fn user_url_analytics_json_export(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(query): Query<AnalyticsQuery>,
) -> Response {
    let (user, _session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    // Ownership check
    let (owner_user_id, target_type) = {
        let conn = state.system_db.lock().unwrap();
        match conn.query_row(
            "SELECT owner_user_id, target_type FROM global_slugs WHERE target_id = ?1",
            [&id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        ) {
            Ok(val) => val,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                return StatusCode::NOT_FOUND.into_response();
            }
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    };

    if owner_user_id != user.id || target_type != "url" {
        return StatusCode::FORBIDDEN.into_response();
    }

    let user_dbs = match state.get_user_dbs(user.id) {
        Ok(dbs) => dbs,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    perform_user_json_export(user_dbs, "url", id, query.date_from, query.date_to).await
}

pub async fn user_page_analytics_csv_export(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(query): Query<AnalyticsQuery>,
) -> Response {
    let (user, _session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    // Ownership check
    let (owner_user_id, target_type) = {
        let conn = state.system_db.lock().unwrap();
        match conn.query_row(
            "SELECT owner_user_id, target_type FROM global_slugs WHERE target_id = ?1",
            [&id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        ) {
            Ok(val) => val,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                return StatusCode::NOT_FOUND.into_response();
            }
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    };

    if owner_user_id != user.id || target_type != "page" {
        return StatusCode::FORBIDDEN.into_response();
    }

    let user_dbs = match state.get_user_dbs(user.id) {
        Ok(dbs) => dbs,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    perform_user_csv_export(user_dbs, "page", id, query.date_from, query.date_to).await
}

pub async fn user_page_analytics_json_export(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(query): Query<AnalyticsQuery>,
) -> Response {
    let (user, _session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    // Ownership check
    let (owner_user_id, target_type) = {
        let conn = state.system_db.lock().unwrap();
        match conn.query_row(
            "SELECT owner_user_id, target_type FROM global_slugs WHERE target_id = ?1",
            [&id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        ) {
            Ok(val) => val,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                return StatusCode::NOT_FOUND.into_response();
            }
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    };

    if owner_user_id != user.id || target_type != "page" {
        return StatusCode::FORBIDDEN.into_response();
    }

    let user_dbs = match state.get_user_dbs(user.id) {
        Ok(dbs) => dbs,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    perform_user_json_export(user_dbs, "page", id, query.date_from, query.date_to).await
}
