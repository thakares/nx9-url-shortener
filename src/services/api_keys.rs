use crate::db::Db;
use crate::models::ApiKey;
use crate::error::AppError;

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
