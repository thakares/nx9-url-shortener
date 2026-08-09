use crate::analytics::AnalyticsQueue;
use crate::config::Config;
use crate::db::Db;
use crate::state::AppState;
use crate::web::create_router;
use std::path::PathBuf;
use std::time::Instant;
use tracing::info;

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

pub async fn run(
    host: Option<String>,
    port: Option<u16>,
    data_dir: Option<String>,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(h) = host {
        config.host = h;
    }
    if let Some(p) = port {
        config.port = p;
    }
    if let Some(d) = data_dir {
        config.data_dir = PathBuf::from(d);
    }

    info!("Starting BZOD server on {}:{}", config.host, config.port);
    info!("Database directory: {:?}", config.data_dir);

    // Init DBs
    let db = Db::init(&config)?;

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let mut join_handles = Vec::new();

    // Init Queue
    let (queue, analytics_handle) = AnalyticsQueue::new(db.clone(), 1000, shutdown_rx.clone());
    join_handles.push(("analytics_worker", analytics_handle));

    // Spawn background tasks
    let link_checker_db = db.clone();
    let link_checker_interval = config.link_check_interval_mins;
    let rx = shutdown_rx.clone();
    join_handles.push((
        "link_checker",
        tokio::spawn(async move {
            crate::jobs::run_link_checker(link_checker_db, link_checker_interval, rx).await;
        }),
    ));

    let aggregator_db = db.clone();
    let aggregator_interval = config.aggregation_interval_mins;
    let rx = shutdown_rx.clone();
    join_handles.push((
        "aggregator",
        tokio::spawn(async move {
            crate::jobs::run_aggregator(aggregator_db, aggregator_interval, rx).await;
        }),
    ));

    let retention_db = db.clone();
    let retention_days = config.data_retention_days;
    let rx = shutdown_rx.clone();
    join_handles.push((
        "retention_cleaner",
        tokio::spawn(async move {
            crate::jobs::run_retention_cleaner(retention_db, retention_days, rx).await;
        }),
    ));

    // Spawn optional backup scheduler
    let backup_db = db.clone();
    let backup_config = config.clone();
    let rx = shutdown_rx.clone();
    join_handles.push((
        "backup_scheduler",
        tokio::spawn(async move {
            crate::jobs::backup::run_backup_scheduler(backup_db, backup_config, rx).await;
        }),
    ));

    let expiry_db = db.clone();
    let rx = shutdown_rx.clone();
    join_handles.push((
        "expiry_checker",
        tokio::spawn(async move {
            crate::jobs::run_expiry_checker(expiry_db, rx).await;
        }),
    ));

    let reconcile_db = db.clone();
    let reconcile_interval_hours = {
        let conn = db.system.lock().unwrap();
        conn.query_row(
            "SELECT value FROM settings WHERE key = 'quota_reconcile_interval_hours';",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|val| val.parse::<u64>().ok())
        .unwrap_or(24)
    };
    let rx = shutdown_rx.clone();
    join_handles.push((
        "quota_reconciliation",
        tokio::spawn(async move {
            crate::jobs::run_quota_reconciliation(reconcile_db, reconcile_interval_hours, rx).await;
        }),
    ));

    let state = AppState {
        admin_db: db.admin.clone(),
        content_db: db.content.clone(),
        analytics_db: db.analytics.clone(),
        system_db: db.system.clone(),
        users_db: db.users.clone(),
        user_dbs: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        db: db.clone(),
        config: config.clone(),
        analytics_queue: queue,
        start_time: Instant::now(),
    };

    // Run axum server
    let router = create_router(state);
    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!("Listening for requests on http://{}", addr);

    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            info!("Shutdown signal received");
            info!("Stopping HTTP server...");
            let _ = shutdown_tx.send(true);
        })
        .await?;

    info!("Stopping background workers...");

    let timeout_duration = std::time::Duration::from_secs(10);
    let deadline = tokio::time::Instant::now() + timeout_duration;

    for (name, handle) in join_handles {
        match tokio::time::timeout_at(deadline, handle).await {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => tracing::error!("Background task '{}' panicked: {:?}", name, e),
            Err(_) => tracing::warn!("Background task did not terminate: {}", name),
        }
    }

    info!("Background workers stopped");
    info!("BZOD shutdown complete");

    Ok(())
}
