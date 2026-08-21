use crate::error::AppError;
use crate::models::LandingPage;

pub fn create_landing_page(
    conn: &rusqlite::Connection,
    code: &str,
    slug: &str,
    title: &str,
    html_content: &str,
    state: &str,
) -> Result<LandingPage, AppError> {
    let page =
        crate::db::content::create_landing_page(conn, code, slug, title, html_content, state)?;
    Ok(page)
}

pub fn get_landing_page_by_code(
    conn: &rusqlite::Connection,
    code: &str,
) -> Result<Option<LandingPage>, AppError> {
    let page = crate::db::content::get_landing_page_by_code(conn, code)?;
    Ok(page)
}
