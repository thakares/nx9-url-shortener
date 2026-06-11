use crate::db::Db;
use crate::models::LandingPage;
use crate::error::AppError;

pub fn create_landing_page(
    db: &Db,
    code: &str,
    slug: &str,
    title: &str,
    html_content: &str,
    state: &str,
) -> Result<LandingPage, AppError> {
    let conn = db.content.lock().unwrap();
    let page = crate::db::content::create_landing_page(&conn, code, slug, title, html_content, state)?;
    Ok(page)
}

pub fn get_landing_page_by_code(db: &Db, code: &str) -> Result<Option<LandingPage>, AppError> {
    let conn = db.content.lock().unwrap();
    let page = crate::db::content::get_landing_page_by_code(&conn, code)?;
    Ok(page)
}
