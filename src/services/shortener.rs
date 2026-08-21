use crate::error::AppError;
use crate::models::Url;

pub fn create_url(
    conn: &rusqlite::Connection,
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
    let url = crate::db::content::create_url(conn, code, destination, title, description, tags)?;
    Ok(url)
}

pub fn get_url_by_code(conn: &rusqlite::Connection, code: &str) -> Result<Option<Url>, AppError> {
    let url = crate::db::content::get_url_by_code(conn, code)?;
    Ok(url)
}
