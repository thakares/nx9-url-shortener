use crate::state::AppState;
use crate::web::{admin, api, bulk, multi_user, pages, password_gate, qr, redirect, system};
use axum::{
    routing::{delete, get, post, put},
    Router,
};

pub fn create_router(state: AppState) -> Router {
    Router::new()
        // --- Root Landing Page ---
        .route("/", get(pages::root_landing))
        // --- Public Redirection Routes ---
        .route("/:code", get(redirect::resolve_redirect))
        .route("/p/:code", get(pages::resolve_page))
        .route("/p/:code/*slug", get(pages::resolve_page))
        .route(
            "/gate/:code",
            get(password_gate::gate_get).post(password_gate::gate_post),
        )
        // --- QR Code Serving ---
        .route("/api/qr/:file", get(qr::qr_handler))
        // --- System Health & Diagnostics ---
        .route("/status", get(system::status_endpoint))
        .route("/metrics", get(system::metrics_endpoint))
        // --- Public User Login ---
        .route(
            "/login",
            get(admin::public_login_get).post(admin::public_login_post),
        )
        .route("/logout", get(admin::public_logout))
        .route("/user/dashboard", get(admin::user_dashboard_get))
        .route("/user/urls", get(admin::user_urls_get))
        .route("/user/urls/create", post(admin::user_urls_create))
        .route("/user/urls/delete/:id", post(admin::user_urls_delete))
        .route("/user/audit", get(admin::user_audit_get))
        .route("/user/status", get(admin::user_status_get))
        .route("/user/pages", get(admin::user_pages_get))
        .route("/user/pages/create", post(admin::user_pages_create))
        .route("/user/pages/delete/:id", post(admin::user_pages_delete))
        .route("/user/settings", get(admin::user_settings_get))
        .route(
            "/user/settings/password",
            post(admin::user_change_password_post),
        )
        .route("/user/settings/backup", get(admin::user_download_backup))
        .route(
            "/user/settings/restore",
            post(admin::user_restore_backup_post),
        )
        .route("/analytics", get(admin::user_analytics_get))
        .route(
            "/user/analytics/url/:id",
            get(admin::user_url_analytics_get),
        )
        .route(
            "/user/analytics/page/:id",
            get(admin::user_page_analytics_get),
        )
        .route("/api-tokens", get(admin::api_tokens_get))
        .route("/api-tokens/create", post(admin::api_tokens_create_post))
        .route(
            "/api-tokens/revoke/:id",
            post(admin::api_tokens_revoke_post),
        )
        // --- Admin UI Login/Logout ---
        .route("/admin", get(admin::admin_index))
        .route(
            "/admin/login",
            get(admin::login_get).post(admin::login_post),
        )
        .route("/admin/logout", get(admin::logout))
        // --- Admin UI Pages ---
        .route("/admin/dashboard", get(admin::dashboard_get))
        .route("/admin/urls", get(admin::urls_get))
        .route("/admin/urls/create", post(admin::urls_create))
        .route("/admin/urls/delete/:id", post(admin::urls_delete))
        .route("/admin/pages", get(admin::pages_get))
        .route("/admin/pages/create", post(admin::pages_create))
        .route("/admin/pages/delete/:id", post(admin::pages_delete))
        .route("/admin/analytics/url/:id", get(admin::url_analytics_get))
        .route("/deploy.sh", get(pages::deploy_script))
        .route(
            "/admin/analytics/url/:id/export/csv",
            get(admin::url_analytics_csv_export),
        )
        .route(
            "/admin/analytics/url/:id/export/json",
            get(admin::url_analytics_json_export),
        )
        .route("/admin/analytics/page/:id", get(admin::page_analytics_get))
        .route(
            "/admin/analytics/page/:id/export/csv",
            get(admin::page_analytics_csv_export),
        )
        .route(
            "/admin/analytics/page/:id/export/json",
            get(admin::page_analytics_json_export),
        )
        .route("/admin/settings", get(admin::settings_get))
        .route("/admin/users", get(admin::users_get))
        .route("/admin/users/new", get(admin::users_new_get))
        .route("/admin/users/:id", get(admin::user_detail_get))
        .route(
            "/admin/users/:id/edit",
            get(admin::user_edit_get).post(admin::user_edit_post),
        )
        .route("/admin/users/create", post(admin::users_create_post))
        .route(
            "/admin/users/status/:id",
            post(admin::users_update_status_post),
        )
        .route("/admin/users/type/:id", post(admin::users_update_type_post))
        .route(
            "/admin/users/password/:id",
            post(admin::users_reset_password_post),
        )
        .route("/admin/users/delete/:id", post(admin::users_delete_post))
        .route(
            "/admin/settings/password",
            post(admin::change_password_post),
        )
        .route(
            "/admin/settings/retention",
            post(admin::change_retention_post),
        )
        .route("/admin/settings/compact", post(admin::compact_db_post))
        .route("/admin/settings/backup", get(admin::download_backup))
        .route("/admin/settings/restore", post(admin::restore_backup_post))
        .route("/admin/settings/bulk-qr", post(admin::bulk_qr_export_post))
        .route(
            "/admin/settings/api-keys/create",
            post(admin::create_api_key_post),
        )
        .route(
            "/admin/settings/api-keys/revoke/:id",
            post(admin::revoke_api_key_post),
        )
        .route("/admin/audit", get(admin::audit_get))
        .route("/admin/status", get(admin::status_get))
        .route(
            "/admin/moderation",
            get(admin::moderation_get).post(admin::moderation_post),
        )
        .route("/admin/slugs", get(admin::slugs_get))
        .route("/admin/slugs/transfer", post(admin::slugs_transfer_post))
        .route("/admin/slugs/status", post(admin::slugs_status_post))
        .route("/admin/slugs/delete", post(admin::slugs_delete_post))
        .route("/admin/sessions", get(admin::sessions_get))
        .route(
            "/admin/sessions/revoke/:id",
            post(admin::sessions_revoke_post),
        )
        .route(
            "/admin/sessions/revoke-all",
            post(admin::sessions_revoke_all_post),
        )
        .route(
            "/admin/quotas",
            get(admin::quotas_get).post(admin::quotas_post),
        )
        .route("/admin/health", get(admin::health_get))
        .route("/admin/backups", get(admin::backups_get))
        .route("/admin/backups/create", post(admin::backups_create_post))
        .route(
            "/admin/backups/download/:filename",
            get(admin::backups_download_get),
        )
        .route(
            "/admin/backups/delete/:filename",
            post(admin::backups_delete_post),
        )
        .route("/admin/backups/restore", post(admin::backups_restore_post))
        // --- REST API v1 JSON Endpoints ---
        .route(
            "/api/v1/urls",
            post(api::api_create_url).get(api::api_list_urls),
        )
        .route(
            "/api/v1/urls/:uuid",
            get(api::api_get_url)
                .put(api::api_update_url)
                .delete(api::api_delete_url),
        )
        .route(
            "/api/v1/pages",
            post(api::api_create_page).get(api::api_list_pages),
        )
        .route(
            "/api/v1/pages/:uuid",
            get(api::api_get_page)
                .put(api::api_update_page)
                .delete(api::api_delete_page),
        )
        .route("/api/v1/stats", get(api::api_overall_stats))
        .route("/api/v1/stats/url/:uuid", get(api::api_url_stats))
        .route("/api/v1/stats/page/:uuid", get(api::api_page_stats))
        // --- Feature Expansion REST API Endpoints ---
        .route("/api/v1/qr/:code", get(api::api_get_qr_stats))
        .route("/api/v1/bulk/qr", post(bulk::api_bulk_qr))
        .route("/api/v1/bulk/url", post(bulk::api_bulk_url))
        .route("/api/v1/audit", get(api::api_list_audit))
        // --- Feature 10 Non-Versioned API Extensions ---
        .route("/api/qr", post(api::api_create_qr))
        .route("/api/bulk/qr", post(bulk::api_bulk_qr))
        .route("/api/bulk/url", post(bulk::api_bulk_url))
        .route("/api/stats", get(api::api_overall_stats))
        .route("/api/audit", get(api::api_list_audit))
        .route(
            "/api/v1/urls/:uuid/preview",
            post(api::api_set_preview)
                .get(api::api_get_preview)
                .delete(api::api_delete_preview),
        )
        .route(
            "/api/v1/urls/:uuid/password",
            post(api::api_set_password).delete(api::api_remove_password),
        )
        // --- Multi-User REST API v1 Admin Endpoints ---
        .route(
            "/api/v1/admin/users",
            get(multi_user::admin_list_users).post(multi_user::admin_create_user),
        )
        .route(
            "/api/v1/admin/users/:id/status",
            put(multi_user::admin_update_user_status),
        )
        .route(
            "/api/v1/admin/users/:id/quotas",
            put(multi_user::admin_update_user_quotas),
        )
        .route(
            "/api/v1/admin/users/:id/password",
            post(multi_user::admin_reset_user_password),
        )
        .route(
            "/api/v1/admin/users/:id",
            delete(multi_user::admin_delete_user),
        )
        .route(
            "/api/v1/admin/transfers",
            post(multi_user::admin_transfer_slug),
        )
        .route(
            "/api/v1/admin/moderation",
            post(multi_user::admin_moderate_slug),
        )
        .route(
            "/api/v1/admin/moderation/events",
            get(multi_user::admin_list_moderation_events),
        )
        // --- Multi-User REST API v1 Tenant User Endpoints ---
        .route("/api/v1/user/profile", get(multi_user::user_get_profile))
        .route(
            "/api/v1/user/password",
            post(multi_user::user_change_password),
        )
        .route(
            "/api/v1/user/api-tokens",
            get(multi_user::user_list_api_tokens).post(multi_user::user_create_api_token),
        )
        .route(
            "/api/v1/user/api-tokens/:id",
            delete(multi_user::user_delete_api_token),
        )
        // --- Static Asset Stub ---
        .route(
            "/static/style.css",
            get(|| async { ([(axum::http::header::CONTENT_TYPE, "text/css")], "") }),
        )
        .with_state(state)
}
