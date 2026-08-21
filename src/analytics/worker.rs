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
        "Flushing {} visits to tenant analytics databases",
        batch.len()
    );

    // Group visits by owner_tenant_id (or legacy user 1 fallback)
    let mut groups: std::collections::HashMap<crate::identity::TenantId, Vec<VisitRecord>> =
        std::collections::HashMap::new();
    let mut legacy_user1_visits: Vec<VisitRecord> = Vec::new();

    for record in batch.drain(..) {
        if let Some(tenant_id) = record.owner_tenant_id {
            groups.entry(tenant_id).or_default().push(record);
        } else if record.owner_user_id == Some(1) {
            legacy_user1_visits.push(record);
        } else {
            error!("Dropping analytics visit with no owner_tenant_id");
        }
    }

    for (tenant_id, tenant_visits) in groups {
        let db_path = db.topology.tenant_analytics_db(tenant_id);
        if !db_path.exists() {
            error!(
                "Refusing to write analytics for non-existent tenant analytics path: {:?}",
                db_path
            );
            continue;
        }

        match rusqlite::Connection::open(&db_path) {
            Ok(mut conn) => {
                let _ = crate::db::sqlite::enable_wal(&conn, "analytics");
                let _ = crate::db::sqlite::enable_foreign_keys(&conn, "analytics");

                if let Err(e) = insert_visits_batch(&mut conn, &tenant_visits) {
                    error!(
                        "Failed to write analytics batch to tenant {} database: {:?}",
                        tenant_id, e
                    );
                }
            }
            Err(e) => {
                error!(
                    "Failed to open analytics database for tenant {}: {:?}",
                    tenant_id, e
                );
            }
        }
    }

    if !legacy_user1_visits.is_empty() {
        if let Ok(legacy_db_path) = db.topology.analytics_db("1") {
            if legacy_db_path.exists() {
                if let Ok(mut conn) = rusqlite::Connection::open(&legacy_db_path) {
                    let _ = crate::db::sqlite::enable_wal(&conn, "analytics");
                    let _ = crate::db::sqlite::enable_foreign_keys(&conn, "analytics");
                    let _ = insert_visits_batch(&mut conn, &legacy_user1_visits);
                }
            }
        }
    }
}
