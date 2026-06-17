use crate::models::Url;
use askama::Template;
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};

#[derive(Template)]
#[template(path = "urls.html")]
pub struct UrlsTemplate {
    pub admin_username: String,
    pub urls: Vec<Url>,
    pub csrf_token: String,
    pub error: Option<String>,
    pub tag_filter: Option<String>,
    pub base_url: String,
    pub current_page: usize,
    pub total_pages: usize,
    pub visible_pages: Vec<usize>,
}

impl UrlsTemplate {
    pub fn is_current(&self, page: &usize) -> bool {
        *page == self.current_page
    }
}

impl IntoResponse for UrlsTemplate {
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
