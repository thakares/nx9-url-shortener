use axum::{
    extract::{Path, State},
    response::{IntoResponse, Redirect, Response},
    Form,
};
use axum_extra::extract::{cookie::Cookie, CookieJar};
use serde::Deserialize;

use crate::auth::password::verify_password;
use crate::services::shortener::get_url_by_code;
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

// POST /gate/:code
pub async fn gate_post(
    State(state): State<AppState>,
    Path(code): Path<String>,
    jar: CookieJar,
    Form(form): Form<PasswordGateForm>,
) -> Response {
    let url_opt = match get_url_by_code(&state.db, &code) {
        Ok(url) => url,
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
            )
                .into_response()
        }
    };

    let url = match url_opt {
        Some(u) => u,
        None => return (axum::http::StatusCode::NOT_FOUND, "Url not found").into_response(),
    };

    let password_hash = match url.password_hash {
        Some(ref h) => h,
        None => {
            // Not password protected, redirect to resolution
            return Redirect::temporary(&format!("/{}", code)).into_response();
        }
    };

    if verify_password(&form.password, password_hash) {
        // Correct password - set 15 min temporary cookie
        let cookie_name = format!("bzod_gate_{}", code);
        let cookie = Cookie::build((cookie_name, "authorized"))
            .secure(state.config.cookie_secure)
            .same_site(axum_extra::extract::cookie::SameSite::Strict)
            .http_only(true)
            .path("/")
            .max_age(time::Duration::minutes(15));

        let updated_jar = jar.add(cookie);
        (updated_jar, Redirect::temporary(&format!("/{}", code))).into_response()
    } else {
        // Invalid password
        GateTemplate {
            code,
            error: Some("Invalid password".to_string()),
        }
        .into_response()
    }
}
