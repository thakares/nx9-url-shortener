use askama::Template;
use axum::{
    response::{IntoResponse, Response, Html},
    http::StatusCode,
};
use crate::models::LandingPage;

#[derive(Template)]
#[template(path = "pages.html")]
pub struct PagesTemplate {
    pub admin_username: String,
    pub pages: Vec<LandingPage>,
    pub csrf_token: String,
    pub error: Option<String>,
}

impl IntoResponse for PagesTemplate {
    fn into_response(self) -> Response {
        match self.render() {
            Ok(html) => Html(html).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Render error: {}", e)).into_response(),
        }
    }
}
