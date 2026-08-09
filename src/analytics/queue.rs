use crate::db::Db;
use crate::models::VisitRecord;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct AnalyticsQueue {
    sender: mpsc::Sender<VisitRecord>,
}

impl AnalyticsQueue {
    pub fn new(
        db: Db,
        capacity: usize,
        shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) -> (Self, tokio::task::JoinHandle<()>) {
        let (sender, receiver) = mpsc::channel(capacity);

        // Spawn background worker to batch-write records
        let handle = tokio::spawn(async move {
            super::worker::run_worker(db, receiver, shutdown_rx).await;
        });

        (Self { sender }, handle)
    }

    // Attempt to queue a visit. Non-blocking.
    pub fn push(&self, record: VisitRecord) {
        use tracing::error;
        if let Err(e) = self.sender.try_send(record) {
            error!("Failed to queue analytics record: {:?}", e);
        }
    }
}
