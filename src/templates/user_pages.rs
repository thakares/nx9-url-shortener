use crate::models::LandingPage;
use askama::Template;
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};

#[derive(Template)]
#[template(path = "user_pages.html")]
pub struct UserPagesTemplate {
    pub admin_username: String,
    pub username: String,
    pub pages: Vec<LandingPage>,
    pub csrf_token: String,
    pub error: Option<String>,
    pub current_page: usize,
    pub total_pages: usize,
    pub visible_pages: Vec<usize>,
}

impl UserPagesTemplate {
    pub fn is_current(&self, page: &usize) -> bool {
        *page == self.current_page
    }
}

impl IntoResponse for UserPagesTemplate {
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
