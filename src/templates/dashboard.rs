use askama::Template;
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};

#[derive(Template)]
#[template(path = "dashboard.html")]
pub struct DashboardTemplate {
    pub admin_username: String,
    pub total_urls: i64,
    pub total_pages: i64,
    pub total_clicks: i64,
    pub active_links: i64,
    pub dead_links: i64,
    pub traffic_chart: String,
    pub countries_chart: String,
    pub browsers_chart: String,
    pub referrers_chart: String,
}

impl IntoResponse for DashboardTemplate {
    fn into_response(self) -> Response {
        match self.render() {
            Ok(html) => Html(html).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Render error: {}", e),
            )
                .into_response(),
        }
    }
}
