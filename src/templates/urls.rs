use askama::Template;
use axum::{
    response::{IntoResponse, Response, Html},
    http::StatusCode,
};
use crate::models::Url;

#[derive(Template)]
#[template(path = "urls.html")]
pub struct UrlsTemplate {
    pub admin_username: String,
    pub urls: Vec<Url>,
    pub csrf_token: String,
    pub error: Option<String>,
    pub tag_filter: Option<String>,
}

impl IntoResponse for UrlsTemplate {
    fn into_response(self) -> Response {
        match self.render() {
            Ok(html) => Html(html).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Render error: {}", e)).into_response(),
        }
    }
}
