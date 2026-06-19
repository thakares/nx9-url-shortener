pub mod analytics;
pub mod dashboard;
pub mod pages;
pub mod settings;
pub mod stats;
pub mod urls;
pub mod user_dashboard;
pub mod user_pages;
pub mod user_settings;
pub mod user_urls;
pub mod users;

pub use analytics::{PageAnalyticsTemplate, UrlAnalyticsTemplate, VisitorLogEntry};
use askama::Template;
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
pub use dashboard::DashboardTemplate;
pub use pages::PagesTemplate;
pub use settings::SettingsTemplate;
pub use stats::{AuditTemplate, StatusTemplate, UserAuditTemplate, UserStatusTemplate};
pub use urls::UrlsTemplate;
pub use user_dashboard::UserDashboardTemplate;
pub use user_pages::UserPagesTemplate;
pub use user_settings::UserSettingsTemplate;
pub use user_urls::UserUrlsTemplate;
pub use users::UsersTemplate;

#[derive(Template)]
#[template(path = "login.html")]
pub struct LoginTemplate {
    pub error: Option<String>,
    pub csrf_token: String,
    pub action: String,
    pub title: String,
    pub subtitle: String,
    pub button_text: String,
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

#[derive(Template)]
#[template(path = "users_new.html")]
pub struct UsersNewTemplate {
    pub admin_username: String,
    pub csrf_token: String,
    pub success: Option<String>,
    pub error: Option<String>,
}

impl IntoResponse for UsersNewTemplate {
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
#[template(path = "user_detail.html")]
pub struct UserDetailTemplate {
    pub admin_username: String,
    pub target_user: crate::models::TenantUser,
    pub stats: crate::web::admin::UserDetailStats,
    pub sessions: Vec<crate::models::UserSession>,
    pub tokens: Vec<crate::models::UserApiToken>,
    pub csrf_token: String,
    pub success: Option<String>,
    pub error: Option<String>,
}

impl IntoResponse for UserDetailTemplate {
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
#[template(path = "user_edit.html")]
pub struct UserEditTemplate {
    pub admin_username: String,
    pub target_user: crate::models::TenantUser,
    pub quotas: crate::models::UserQuotas,
    pub csrf_token: String,
    pub success: Option<String>,
    pub error: Option<String>,
}

impl IntoResponse for UserEditTemplate {
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
#[template(path = "moderation.html")]
pub struct ModerationTemplate {
    pub admin_username: String,
    pub flagged_items: Vec<crate::web::admin::GlobalSlugRow>,
    pub logs: Vec<crate::web::admin::ModerationLogEntry>,
    pub csrf_token: String,
    pub success: Option<String>,
    pub error: Option<String>,
}

impl IntoResponse for ModerationTemplate {
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
#[template(path = "slugs.html")]
pub struct SlugsTemplate {
    pub admin_username: String,
    pub slugs: Vec<crate::web::admin::GlobalSlugRow>,
    pub history: Vec<crate::web::admin::SlugHistoryRow>,
    pub csrf_token: String,
    pub search_filter: Option<String>,
    pub owner_filter: Option<i64>,
    pub status_filter: Option<String>,
    pub success: Option<String>,
    pub error: Option<String>,
}

impl IntoResponse for SlugsTemplate {
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
#[template(path = "sessions.html")]
pub struct SessionsTemplate {
    pub admin_username: String,
    pub sessions: Vec<crate::models::UserSession>,
    pub csrf_token: String,
    pub success: Option<String>,
    pub error: Option<String>,
}

impl IntoResponse for SessionsTemplate {
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
#[template(path = "quotas.html")]
pub struct QuotasTemplate {
    pub admin_username: String,
    pub quotas: Vec<crate::models::UserQuotas>,
    pub csrf_token: String,
    pub success: Option<String>,
    pub error: Option<String>,
}

impl IntoResponse for QuotasTemplate {
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
#[template(path = "health.html")]
pub struct HealthTemplate {
    pub admin_username: String,
    pub db_reports: Vec<crate::db::sqlite::DatabaseHealthReport>,
    pub total_data_size: String,
    pub system_db_size: String,
    pub users_db_size: String,
    pub admin_db_size: String,
    pub tenants_db_size: String,
    pub job_history: Vec<crate::web::admin::JobHistoryRow>,
    pub health_checks: Vec<crate::web::admin::HealthCheckRow>,
    pub csrf_token: String,
    pub success: Option<String>,
    pub error: Option<String>,
}

impl IntoResponse for HealthTemplate {
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
#[template(path = "backups.html")]
pub struct BackupsTemplate {
    pub admin_username: String,
    pub files: Vec<crate::web::admin::BackupFileRow>,
    pub history: Vec<crate::web::admin::BackupHistoryRow>,
    pub csrf_token: String,
    pub success: Option<String>,
    pub error: Option<String>,
}

impl IntoResponse for BackupsTemplate {
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
#[template(path = "api_tokens.html")]
pub struct ApiTokensTemplate {
    pub admin_username: String,
    pub username: String,
    pub tokens: Vec<crate::models::UserApiToken>,
    pub new_token: Option<String>,
    pub csrf_token: String,
    pub success: Option<String>,
    pub error: Option<String>,
}

impl IntoResponse for ApiTokensTemplate {
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
#[template(path = "user_analytics.html")]
pub struct UserAnalyticsTemplate {
    pub admin_username: String,
    pub username: String,
    pub total_clicks: i64,
    pub unique_visitors: i64,
    pub direct_clicks: i64,
    pub referred_clicks: i64,
    pub referrers_chart: String,
    pub browsers_chart: String,
    pub visits: Vec<crate::models::VisitRecord>,
    pub csrf_token: String,
    pub success: Option<String>,
    pub error: Option<String>,
}

impl IntoResponse for UserAnalyticsTemplate {
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
#[template(path = "user_url_analytics.html")]
pub struct UserUrlAnalyticsTemplate {
    pub admin_username: String,
    pub username: String,
    pub url_code: String,
    pub destination: String,
    pub visits: Vec<VisitorLogEntry>,
}

impl IntoResponse for UserUrlAnalyticsTemplate {
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
#[template(path = "user_page_analytics.html")]
pub struct UserPageAnalyticsTemplate {
    pub admin_username: String,
    pub username: String,
    pub page_code: String,
    pub title: String,
    pub visits: Vec<VisitorLogEntry>,
}

impl IntoResponse for UserPageAnalyticsTemplate {
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
