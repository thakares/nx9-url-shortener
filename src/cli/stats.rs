use crate::config::Config;
use crate::db::Db;
use std::path::PathBuf;

pub async fn run(
    data_dir: Option<String>,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(d) = data_dir {
        config.data_dir = PathBuf::from(d);
    }
    let db = Db::init(&config)?;

    println!("=== BZOD Database Stats ===");
    println!("Storage Directory: {:?}", config.data_dir);

    let files = vec![
        ("admin.db", db.topology.admin_db()),
        ("system.db", db.topology.system_db()),
        ("users.db", db.topology.users_registry_db()),
        ("global_urls.db", db.topology.global_urls_db()),
        (
            "global_landing_pages.db",
            db.topology.global_landing_pages_db(),
        ),
        ("reserved.db", db.topology.reserved_db()),
        (
            "legacy content.db",
            db.topology
                .content_db(crate::db::topology::LEGACY_ADMIN_USER_KEY)?,
        ),
        (
            "legacy analytics.db",
            db.topology
                .analytics_db(crate::db::topology::LEGACY_ADMIN_USER_KEY)?,
        ),
    ];

    for (name, path) in files {
        if path.exists() {
            let sz = std::fs::metadata(&path)?.len();
            println!(
                "  File: {} - Size: {} bytes ({:.2} MB)",
                name,
                sz,
                sz as f64 / 1_048_576.0
            );
        }
    }

    let users_count = {
        let conn = db.admin.lock().unwrap();
        crate::db::admin::get_user_count(&conn)?
    };
    println!("Users Count: {}", users_count);

    let (urls_total, urls_active, urls_dead) = {
        let conn = db.global_urls.lock().unwrap();
        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM global_urls WHERE status != 'retired';",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let active: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM global_urls WHERE status = 'active';",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let dead: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM global_urls WHERE status = 'disabled';",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        (total, active, dead)
    };
    println!(
        "Shortened URLs: {} total ({} active / {} dead)",
        urls_total, urls_active, urls_dead
    );

    let pages_count = {
        let conn = db.global_landing_pages.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM global_landing_pages WHERE status != 'retired';",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0)
    };
    println!("Landing Pages: {}", pages_count);

    let total_visits = {
        let users_conn = db.users.lock().unwrap();
        crate::db::users::get_platform_total_clicks(&db.topology, &users_conn).unwrap_or(0)
    };
    println!("Redirect Clicks: {}", total_visits);

    Ok(())
}
