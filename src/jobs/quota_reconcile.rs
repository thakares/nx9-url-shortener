use crate::db::Db;
use std::time::Duration;
use tracing::{error, info};

pub async fn run_quota_reconciliation(
    db: Db,
    interval_hours: u64,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        // Sleep first
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(interval_hours * 3600)) => {}
            _ = shutdown_rx.changed() => {
                info!("Quota reconciliation shutting down...");
                break;
            }
        }
        info!("Running background quota reconciliation...");

        let user_ids: Vec<i64> = {
            let conn = db.users.lock().unwrap();
            let mut stmt = match conn.prepare("SELECT id FROM users;") {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to prepare select user IDs: {:?}", e);
                    continue;
                }
            };
            let rows = match stmt.query_map([], |row| row.get(0)) {
                Ok(r) => r,
                Err(e) => {
                    error!("Failed to query user IDs: {:?}", e);
                    continue;
                }
            };
            rows.filter_map(|r| r.ok()).collect()
        };

        for user_id in user_ids {
            if let Ok(content_conn) = super::open_user_content_conn(&db, user_id) {
                let users_conn = db.users.lock().unwrap();
                if let Err(e) =
                    crate::db::users::reconcile_user_quotas(&users_conn, user_id, &content_conn)
                {
                    error!("Failed to reconcile quotas for user {}: {:?}", user_id, e);
                }
            }
        }
        info!("Quota reconciliation finished.");
    }
}
