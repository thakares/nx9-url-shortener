use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{interval, MissedTickBehavior};
use tracing::{error, info};

use crate::db::analytics::insert_visits_batch;
use crate::db::Db;
use crate::models::VisitRecord;

pub async fn run_worker(
    db: Db,
    mut receiver: mpsc::Receiver<VisitRecord>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    let mut batch = Vec::new();
    let batch_size = 50;
    let flush_interval = Duration::from_secs(2);

    let mut timer = interval(flush_interval);
    timer.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            record_opt = receiver.recv() => {
                match record_opt {
                    Some(record) => {
                        batch.push(record);
                        if batch.len() >= batch_size {
                            flush_batch(&db, &mut batch);
                        }
                    }
                    None => {
                        info!("Analytics channel closed. Flushing remaining records.");
                        flush_batch(&db, &mut batch);
                        break;
                    }
                }
            }
            _ = timer.tick() => {
                if !batch.is_empty() {
                    flush_batch(&db, &mut batch);
                }
            }
            _ = shutdown_rx.changed() => {
                info!("Analytics worker flushing pending records");
                flush_batch(&db, &mut batch);
                break;
            }
        }
    }
}

fn flush_batch(db: &Db, batch: &mut Vec<VisitRecord>) {
    if batch.is_empty() {
        return;
    }

    info!(
        "Flushing {} visits to user analytics databases",
        batch.len()
    );

    // Group visits by owner_user_id
    let mut groups: std::collections::HashMap<i64, Vec<VisitRecord>> =
        std::collections::HashMap::new();
    for record in batch.drain(..) {
        let user_id = record.owner_user_id.unwrap_or(1); // fallback to legacy_admin (user 1)
        groups.entry(user_id).or_default().push(record);
    }

    for (user_id, user_visits) in groups {
        let db_path = db
            .data_dir
            .join("users")
            .join(user_id.to_string())
            .join("analytics.db");
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        match rusqlite::Connection::open(&db_path) {
            Ok(mut conn) => {
                let _ = crate::db::sqlite::enable_wal(&conn, "analytics");
                let _ = crate::db::sqlite::enable_foreign_keys(&conn, "analytics");

                if let Err(e) = insert_visits_batch(&mut conn, &user_visits) {
                    error!(
                        "Failed to write analytics batch to user {} database: {:?}",
                        user_id, e
                    );
                }
            }
            Err(e) => {
                error!(
                    "Failed to open analytics database for user {}: {:?}",
                    user_id, e
                );
            }
        }
    }
}
