use crate::models::TenantUser;
use askama::Template;
use axum::response::{Html, IntoResponse, Response};

#[derive(Template)]
#[template(path = "users.html")]
pub struct UsersTemplate {
    pub admin_username: String,
    pub users: Vec<TenantUser>,
    pub csrf_token: String,
    pub success: Option<String>,
    pub error: Option<String>,
}

impl IntoResponse for UsersTemplate {
    fn into_response(self) -> Response {
        match self.render() {
            Ok(html) => Html(html).into_response(),
            Err(e) => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Render error: {}", e),
            )
                .into_response(),
        }
    }
}
