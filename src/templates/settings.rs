use askama::Template;
use axum::{
    response::{IntoResponse, Response, Html},
    http::StatusCode,
};
use crate::models::ApiKey;

#[derive(Template)]
#[template(path = "settings.html")]
pub struct SettingsTemplate {
    pub admin_username: String,
    pub api_keys: Vec<ApiKey>,
    pub data_retention: String,
    pub csrf_token: String,
    pub success: Option<String>,
    pub error: Option<String>,
}

impl IntoResponse for SettingsTemplate {
    fn into_response(self) -> Response {
        match self.render() {
            Ok(html) => Html(html).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Render error: {}", e)).into_response(),
        }
    }
}
