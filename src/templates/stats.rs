use askama::Template;
use axum::{
    response::{IntoResponse, Response, Html},
    http::StatusCode,
};
use crate::models::AuditLog;

#[derive(Template)]
#[template(path = "status_ui.html")]
pub struct StatusTemplate {
    pub admin_username: String,
    pub app_status: &'static str,
    pub db_status: String,
    pub queue_size: usize,
    pub memory_usage: String,
    pub uptime: String,
    pub version: &'static str,
    pub git_commit: &'static str,
}

#[derive(Template)]
#[template(path = "audit.html")]
pub struct AuditTemplate {
    pub admin_username: String,
    pub logs: Vec<AuditLog>,
}

impl IntoResponse for StatusTemplate {
    fn into_response(self) -> Response {
        match self.render() {
            Ok(html) => Html(html).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Render error: {}", e)).into_response(),
        }
    }
}

impl IntoResponse for AuditTemplate {
    fn into_response(self) -> Response {
        match self.render() {
            Ok(html) => Html(html).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Render error: {}", e)).into_response(),
        }
    }
}
