use crate::models::LandingPage;
use askama::Template;
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};

#[derive(Template)]
#[template(path = "pages.html")]
pub struct PagesTemplate {
    pub admin_username: String,
    pub pages: Vec<LandingPage>,
    pub csrf_token: String,
    pub error: Option<String>,
    pub current_page: usize,
    pub total_pages: usize,
    pub visible_pages: Vec<usize>,
}

impl PagesTemplate {
    pub fn is_current(&self, page: &usize) -> bool {
        *page == self.current_page
    }
}

impl IntoResponse for PagesTemplate {
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
