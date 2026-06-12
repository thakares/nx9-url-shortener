use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{interval, MissedTickBehavior};
use tracing::{error, info};

use crate::db::analytics::insert_visits_batch;
use crate::db::Db;
use crate::models::VisitRecord;

pub async fn run_worker(db: Db, mut receiver: mpsc::Receiver<VisitRecord>) {
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
        }
    }
}

fn flush_batch(db: &Db, batch: &mut Vec<VisitRecord>) {
    if batch.is_empty() {
        return;
    }

    info!("Flushing {} visits to analytics database", batch.len());
    let mut conn_lock = db.analytics.lock().unwrap();
    if let Err(e) = insert_visits_batch(&mut conn_lock, batch) {
        error!("Failed to write analytics batch to database: {:?}", e);
    } else {
        batch.clear();
    }
}
