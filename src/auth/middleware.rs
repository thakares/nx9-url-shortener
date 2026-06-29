use crate::auth::session::authenticate_api_key;
use crate::models::ApiActor;
use crate::state::AppState;
use axum::{
    extract::{FromRef, FromRequestParts},
    http::{request::Parts, StatusCode},
    Json,
};

// Extractor: Authenticate API requests using Bearer token
pub struct ApiUser(pub ApiActor);

impl ApiUser {
    pub fn require_admin(
        &self,
    ) -> Result<&crate::models::User, (StatusCode, Json<crate::web::api::ApiError>)> {
        match &self.0 {
            ApiActor::Admin(u) => Ok(u),
            _ => Err((
                StatusCode::FORBIDDEN,
                Json(crate::web::api::ApiError {
                    error: "Admin privileges required".to_string(),
                }),
            )),
        }
    }

    pub fn require_tenant(
        &self,
    ) -> Result<&crate::models::TenantUser, (StatusCode, Json<crate::web::api::ApiError>)> {
        match &self.0 {
            ApiActor::User(u) => Ok(u),
            _ => Err((
                StatusCode::FORBIDDEN,
                Json(crate::web::api::ApiError {
                    error: "Tenant privileges required".to_string(),
                }),
            )),
        }
    }
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for ApiUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .ok_or((StatusCode::UNAUTHORIZED, "Missing Authorization header"))?;

        let admin_conn = app_state.admin_db.lock().unwrap();
        let users_conn = app_state.users_db.lock().unwrap();
        match authenticate_api_key(&admin_conn, &users_conn, auth_header) {
            Ok(Some(actor)) => Ok(ApiUser(actor)),
            Ok(None) => Err((StatusCode::UNAUTHORIZED, "Invalid API token")),
            Err(_) => Err((StatusCode::INTERNAL_SERVER_ERROR, "Database error")),
        }
    }
}
