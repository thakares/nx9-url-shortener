pub mod dashboard;
pub mod urls;
pub mod pages;
pub mod stats;
pub mod settings;

pub use dashboard::DashboardTemplate;
pub use urls::UrlsTemplate;
pub use pages::PagesTemplate;
pub use stats::{StatusTemplate, AuditTemplate};
pub use settings::SettingsTemplate;

use askama::Template;
use axum::{
    response::{IntoResponse, Response, Html},
    http::StatusCode,
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
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Render error: {}", e)).into_response(),
        }
    }
}
