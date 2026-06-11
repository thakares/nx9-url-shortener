use tokio::sync::mpsc;
use crate::models::VisitRecord;
use crate::db::Db;

#[derive(Clone)]
pub struct AnalyticsQueue {
    sender: mpsc::Sender<VisitRecord>,
}

impl AnalyticsQueue {
    pub fn new(db: Db, capacity: usize) -> Self {
        let (sender, receiver) = mpsc::channel(capacity);

        // Spawn background worker to batch-write records
        tokio::spawn(async move {
            super::worker::run_worker(db, receiver).await;
        });

        Self { sender }
    }

    // Attempt to queue a visit. Non-blocking.
    pub fn push(&self, record: VisitRecord) {
        use tracing::error;
        if let Err(e) = self.sender.try_send(record) {
            error!("Failed to queue analytics record: {:?}", e);
        }
    }
}
