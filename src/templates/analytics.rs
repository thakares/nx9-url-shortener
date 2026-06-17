use crate::models::{LandingPage, Url};
use askama::Template;
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};

#[derive(Clone, Debug)]
pub struct VisitorLogEntry {
    pub sr: usize,
    pub timestamp: String,
    pub ip_address: String,
    pub country: String,
    pub referrer: String,
    pub browser: String,
    pub user_agent: String,
    pub utm_source: String,
    pub utm_campaign: String,
}

#[derive(Template)]
#[template(path = "url_analytics.html")]
pub struct UrlAnalyticsTemplate {
    pub admin_username: String,
    pub url: Url,
    pub total_clicks: i64,
    pub unique_visitors: i64,
    pub qr_scans: i64,
    pub direct_clicks: i64,
    pub traffic_chart: String,
    pub monthly_chart: String,
    pub countries_chart: String,
    pub referrers_chart: String,
    pub browsers_chart: String,
    // Paginated visitor logs
    pub visits: Vec<VisitorLogEntry>,
    pub current_page: usize,
    pub total_pages: usize,
    pub visible_pages: Vec<usize>,
    pub total_records: i64,
    pub page_start: usize,
    pub page_end: usize,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
}

impl UrlAnalyticsTemplate {
    pub fn is_current(&self, page: &usize) -> bool {
        *page == self.current_page
    }
}


impl IntoResponse for UrlAnalyticsTemplate {
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

#[derive(Template)]
#[template(path = "page_analytics.html")]
pub struct PageAnalyticsTemplate {
    pub admin_username: String,
    pub page: LandingPage,
    pub total_views: i64,
    pub unique_visitors: i64,
    pub traffic_chart: String,
    pub monthly_chart: String,
    pub countries_chart: String,
    pub referrers_chart: String,
    // Paginated visitor logs
    pub visits: Vec<VisitorLogEntry>,
    pub current_page: usize,
    pub total_pages: usize,
    pub visible_pages: Vec<usize>,
    pub total_records: i64,
    pub page_start: usize,
    pub page_end: usize,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
}

impl PageAnalyticsTemplate {
    pub fn is_current(&self, page: &usize) -> bool {
        *page == self.current_page
    }
}


impl IntoResponse for PageAnalyticsTemplate {
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
