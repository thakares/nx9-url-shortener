use crate::db::Db;
use crate::error::AppError;
use crate::models::ApiKey;

pub fn create_api_key(
    db: &Db,
    user_id: &str,
    name: &str,
    key_hash: &str,
) -> Result<ApiKey, AppError> {
    let conn = db.admin.lock().unwrap();
    let key = crate::db::admin::create_api_key(&conn, user_id, name, key_hash)?;
    Ok(key)
}
