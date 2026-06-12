use crate::analytics::AnalyticsQueue;
use crate::config::Config;
use crate::db::Db;
use crate::state::AppState;
use crate::web::create_router;
use std::path::PathBuf;
use std::time::Instant;
use tracing::info;

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

    // Init Queue
    let queue = AnalyticsQueue::new(db.clone(), 1000);

    // Spawn background tasks
    let link_checker_db = db.clone();
    let link_checker_interval = config.link_check_interval_mins;
    tokio::spawn(async move {
        crate::jobs::run_link_checker(link_checker_db, link_checker_interval).await;
    });

    let aggregator_db = db.clone();
    let aggregator_interval = config.aggregation_interval_mins;
    tokio::spawn(async move {
        crate::jobs::run_aggregator(aggregator_db, aggregator_interval).await;
    });

    let retention_db = db.clone();
    let retention_days = config.data_retention_days;
    tokio::spawn(async move {
        crate::jobs::run_retention_cleaner(retention_db, retention_days).await;
    });

    // Spawn optional backup scheduler
    let backup_db = db.clone();
    let backup_config = config.clone();
    tokio::spawn(async move {
        crate::jobs::backup::run_backup_scheduler(backup_db, backup_config).await;
    });

    let expiry_db = db.clone();
    tokio::spawn(async move {
        crate::jobs::run_expiry_checker(expiry_db).await;
    });

    let state = AppState {
        admin_db: db.admin.clone(),
        content_db: db.content.clone(),
        analytics_db: db.analytics.clone(),
        system_db: db.system.clone(),
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
    axum::serve(listener, router).await?;

    Ok(())
}
