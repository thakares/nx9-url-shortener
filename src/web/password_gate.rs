use axum::{
    extract::{Path, State},
    response::{IntoResponse, Redirect, Response},
    Form,
};
use axum_extra::extract::{cookie::Cookie, CookieJar};
use serde::Deserialize;

use crate::auth::password::verify_password;
use crate::db::tenant::TenantOpenMode;
use crate::state::AppState;
use crate::templates::GateTemplate;

#[derive(Deserialize)]
pub struct PasswordGateForm {
    pub password: String,
}

// GET /gate/:code
pub async fn gate_get(Path(code): Path<String>) -> impl IntoResponse {
    GateTemplate { code, error: None }
}

enum GateLookup {
    Missing,
    Gone,
    Unprotected,
    Protected(String),
}

fn lookup_gate(state: &AppState, code: &str) -> Result<GateLookup, axum::http::StatusCode> {
    let info = match state
        .lookup_slug(code)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
    {
        Some(info) if info.status == "active" => info,
        Some(_) => return Ok(GateLookup::Gone),
        None => return Ok(GateLookup::Missing),
    };

    let user_dbs = match state.open_slug_owner(&info.owner_tenant_id, TenantOpenMode::PublicContent)
    {
        Ok(dbs) => dbs,
        Err(_) => return Ok(GateLookup::Missing),
    };
    let conn = user_dbs
        .content
        .lock()
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    match crate::db::content::get_url_by_code(&conn, code)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
    {
        Some(u) => match u.password_hash {
            Some(h) => Ok(GateLookup::Protected(h)),
            None => Ok(GateLookup::Unprotected),
        },
        None => Ok(GateLookup::Missing),
    }
}

// POST /gate/:code
pub async fn gate_post(
    State(state): State<AppState>,
    Path(code): Path<String>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Form(form): Form<PasswordGateForm>,
) -> Response {
    let password_hash = match lookup_gate(&state, &code) {
        Ok(GateLookup::Protected(h)) => h,
        Ok(GateLookup::Unprotected) => {
            return Redirect::temporary(&format!("/{}", code)).into_response();
        }
        Ok(GateLookup::Missing) => {
            return (axum::http::StatusCode::NOT_FOUND, "Url not found").into_response();
        }
        Ok(GateLookup::Gone) => {
            return (
                axum::http::StatusCode::GONE,
                "This content has been disabled or moderated",
            )
                .into_response();
        }
        Err(status) => return (status, "Database error").into_response(),
    };

    if verify_password(&form.password, &password_hash) {
        let cookie_name = format!("bzod_gate_{}", code);
        let secure_flag = crate::utils::resolve_cookie_secure(state.config.cookie_secure, &headers);
        let cookie = Cookie::build((cookie_name, "authorized"))
            .secure(secure_flag)
            .same_site(axum_extra::extract::cookie::SameSite::Strict)
            .http_only(true)
            .path("/")
            .max_age(time::Duration::minutes(15));

        let updated_jar = jar.add(cookie);
        (updated_jar, Redirect::temporary(&format!("/{}", code))).into_response()
    } else {
        GateTemplate {
            code,
            error: Some("Invalid password".to_string()),
        }
        .into_response()
    }
}
