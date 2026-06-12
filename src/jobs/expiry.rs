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

        let count = {
            let conn = db.content.lock().unwrap();
            crate::db::content::expire_urls(&conn).unwrap_or(0)
        };

        if count > 0 {
            info!(expired_count = count, "Expired URLs marked");
        }
    }
}
