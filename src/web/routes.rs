use axum::{
    Router,
    routing::{get, post},
};
use crate::state::AppState;
use crate::web::{redirect, pages, admin, api, system};

pub fn create_router(state: AppState) -> Router {
    Router::new()
        // --- Public Redirection Routes ---
        .route("/:code", get(redirect::resolve_redirect))
        .route("/p/:code", get(pages::resolve_page))
        .route("/p/:code/*slug", get(pages::resolve_page))

        // --- System Health & Diagnostics ---
        .route("/status", get(system::status_endpoint))
        .route("/metrics", get(system::metrics_endpoint))

        // --- Admin UI Login/Logout ---
        .route("/admin", get(admin::admin_index))
        .route("/admin/login", get(admin::login_get).post(admin::login_post))
        .route("/admin/logout", get(admin::logout))

        // --- Admin UI Pages ---
        .route("/admin/dashboard", get(admin::dashboard_get))
        .route("/admin/urls", get(admin::urls_get))
        .route("/admin/urls/create", post(admin::urls_create))
        .route("/admin/urls/delete/:id", post(admin::urls_delete))
        .route("/admin/pages", get(admin::pages_get))
        .route("/admin/pages/create", post(admin::pages_create))
        .route("/admin/pages/delete/:id", post(admin::pages_delete))
        .route("/admin/settings", get(admin::settings_get))
        .route("/admin/settings/password", post(admin::change_password_post))
        .route("/admin/settings/retention", post(admin::change_retention_post))
        .route("/admin/settings/compact", post(admin::compact_db_post))
        .route("/admin/settings/backup", get(admin::download_backup))
        .route("/admin/settings/api-keys/create", post(admin::create_api_key_post))
        .route("/admin/settings/api-keys/revoke/:id", post(admin::revoke_api_key_post))
        .route("/admin/audit", get(admin::audit_get))
        .route("/admin/status", get(admin::status_get))

        // --- REST API v1 JSON Endpoints ---
        .route("/api/v1/urls", post(api::api_create_url).get(api::api_list_urls))
        .route("/api/v1/urls/:uuid", get(api::api_get_url).put(api::api_update_url).delete(api::api_delete_url))
        .route("/api/v1/pages", post(api::api_create_page).get(api::api_list_pages))
        .route("/api/v1/pages/:uuid", get(api::api_get_page).put(api::api_update_page).delete(api::api_delete_page))
        .route("/api/v1/stats", get(api::api_overall_stats))
        .route("/api/v1/stats/url/:uuid", get(api::api_url_stats))
        .route("/api/v1/stats/page/:uuid", get(api::api_page_stats))

        // --- Static Asset Stub ---
        .route("/static/style.css", get(|| async { ([(axum::http::header::CONTENT_TYPE, "text/css")], "") }))

        .with_state(state)
}
