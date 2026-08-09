use crate::db::Db;
use std::time::Duration;
use tracing::{error, info, warn};

/// Background job that marks expired URLs.
///
/// Runs every 60 seconds. Any URL with `expires_at < NOW()` and `expired = 0`
/// gets flipped to `expired = 1`.
///
/// Correctness note: the redirect handler treats wall-clock `expires_at` as
/// authoritative and returns 410 without depending on this sweeper. The sweeper
/// is maintenance (persist `expired=1`) and must remain idempotent.
pub async fn run_expiry_checker(db: Db, mut shutdown_rx: tokio::sync::watch::Receiver<bool>) {
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(60)) => {}
            _ = shutdown_rx.changed() => {
                info!("Expiry checker shutting down...");
                break;
            }
        }

        let user_ids: Vec<i64> = {
            let conn = match db.users.lock() {
                Ok(c) => c,
                Err(e) => {
                    error!(error = %e, "expiry job: users_db mutex poisoned");
                    continue;
                }
            };
            let mut stmt = match conn.prepare("SELECT id FROM users;") {
                Ok(s) => s,
                Err(e) => {
                    error!(error = %e, "expiry job: failed to list users");
                    continue;
                }
            };
            let rows = match stmt.query_map([], |row| row.get(0)) {
                Ok(r) => r,
                Err(e) => {
                    error!(error = %e, "expiry job: failed to map user ids");
                    continue;
                }
            };
            rows.filter_map(|r| r.ok()).collect()
        };

        let mut total_expired = 0;
        for user_id in user_ids {
            match super::open_user_content_conn(&db, user_id) {
                Ok(conn) => match crate::db::content::expire_urls(&conn) {
                    Ok(count) => total_expired += count,
                    Err(e) => {
                        warn!(
                            owner_user_id = user_id,
                            error = %e,
                            "expiry job: expire_urls failed"
                        );
                    }
                },
                Err(e) => {
                    // Missing content.db for a user is common; only log open errors that are unexpected.
                    if !matches!(e, rusqlite::Error::SqliteFailure(_, _)) {
                        warn!(
                            owner_user_id = user_id,
                            error = %e,
                            "expiry job: could not open content.db"
                        );
                    }
                }
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
