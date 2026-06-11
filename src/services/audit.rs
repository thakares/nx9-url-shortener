use crate::db::Db;
use crate::models::AuditLog;
use crate::error::AppError;

pub fn log_action(
    db: &Db,
    username: &str,
    action: &str,
    object_type: Option<&str>,
    object_id: Option<&str>,
    ip_address: Option<&str>,
    user_agent: Option<&str>,
) -> Result<AuditLog, AppError> {
    let conn = db.admin.lock().unwrap();
    let log = crate::db::admin::write_audit_log(
        &conn,
        username,
        action,
        object_type,
        object_id,
        ip_address,
        user_agent,
    )?;
    Ok(log)
}
