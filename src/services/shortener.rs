use crate::db::Db;
use crate::error::AppError;
use crate::models::Url;

pub fn create_url(
    db: &Db,
    code: &str,
    destination: &str,
    title: Option<&str>,
    description: Option<&str>,
    tags: &[String],
) -> Result<Url, AppError> {
    if !crate::utils::validation::validate_redirect_destination(destination) {
        return Err(AppError::BadRequest(
            "Destination must be a valid http(s) URL without control characters".into(),
        ));
    }
    let conn = db
        .content
        .lock()
        .map_err(|e| AppError::Internal(format!("content_db mutex poisoned: {}", e)))?;
    let url = crate::db::content::create_url(&conn, code, destination, title, description, tags)?;
    Ok(url)
}

pub fn get_url_by_code(db: &Db, code: &str) -> Result<Option<Url>, AppError> {
    let conn = db
        .content
        .lock()
        .map_err(|e| AppError::Internal(format!("content_db mutex poisoned: {}", e)))?;
    let url = crate::db::content::get_url_by_code(&conn, code)?;
    Ok(url)
}
