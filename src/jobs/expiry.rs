use crate::db::Db;
use std::time::Duration;
use tracing::info;

/// Background job that marks expired URLs.
///
/// Runs every 60 seconds. Any URL with `expires_at < NOW()` and `expired = 0`
/// gets flipped to `expired = 1`.
pub async fn run_expiry_checker(db: Db) {
    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;

        let user_ids: Vec<i64> = {
            let conn = db.users.lock().unwrap();
            let mut stmt = match conn.prepare("SELECT id FROM users;") {
                Ok(s) => s,
                Err(_) => continue,
            };
            let rows = match stmt.query_map([], |row| row.get(0)) {
                Ok(r) => r,
                Err(_) => continue,
            };
            rows.filter_map(|r| r.ok()).collect()
        };

        let mut total_expired = 0;
        for user_id in user_ids {
            if let Ok(conn) = super::open_user_content_conn(&db, user_id) {
                let count = crate::db::content::expire_urls(&conn).unwrap_or(0);
                total_expired += count;
            }
        }

        if total_expired > 0 {
            info!(
                expired_count = total_expired,
                "Expired URLs marked across users"
            );
        }
    }
}
