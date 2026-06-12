pub mod dashboard;
pub mod pages;
pub mod settings;
pub mod stats;
pub mod urls;

pub use dashboard::DashboardTemplate;
pub use pages::PagesTemplate;
pub use settings::SettingsTemplate;
pub use stats::{AuditTemplate, StatusTemplate};
pub use urls::UrlsTemplate;

use askama::Template;
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};

#[derive(Template)]
#[template(path = "login.html")]
pub struct LoginTemplate {
    pub error: Option<String>,
    pub csrf_token: String,
}

impl IntoResponse for LoginTemplate {
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
#[template(path = "gate.html")]
pub struct GateTemplate {
    pub code: String,
    pub error: Option<String>,
}

impl IntoResponse for GateTemplate {
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
#[template(path = "preview.html")]
pub struct PreviewTemplate {
    pub code: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub logo_url: Option<String>,
    pub button_text: String,
    pub destination: String,
}

impl IntoResponse for PreviewTemplate {
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
