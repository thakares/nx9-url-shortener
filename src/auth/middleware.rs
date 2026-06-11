use axum::{
    extract::{FromRequestParts, FromRef},
    http::{request::Parts, StatusCode},
};
use crate::state::AppState;
use crate::models::User;
use crate::auth::session::authenticate_api_key;

// Extractor: Authenticate API requests using Bearer token
pub struct ApiUser(pub User);

#[axum::async_trait]
impl<S> FromRequestParts<S> for ApiUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);
        let auth_header = parts.headers
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .ok_or((StatusCode::UNAUTHORIZED, "Missing Authorization header"))?;

        let conn = app_state.admin_db.lock().unwrap();
        match authenticate_api_key(&conn, auth_header) {
            Ok(Some(user)) => Ok(ApiUser(user)),
            Ok(None) => Err((StatusCode::UNAUTHORIZED, "Invalid API token")),
            Err(_) => Err((StatusCode::INTERNAL_SERVER_ERROR, "Database error")),
        }
    }
}
