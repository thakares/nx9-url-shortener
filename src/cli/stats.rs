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
        ("admin.db", config.data_dir.join("admin/admin.db")),
        ("system.db", config.data_dir.join("admin/system.db")),
        ("users.db", config.data_dir.join("admin/users.db")),
        (
            "legacy content.db",
            config.data_dir.join("users/1/content.db"),
        ),
        (
            "legacy analytics.db",
            config.data_dir.join("users/1/analytics.db"),
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
        let conn = db.content.lock().unwrap();
        crate::db::content::get_url_counts(&conn)?
    };
    println!(
        "Shortened URLs: {} total ({} active / {} dead)",
        urls_total, urls_active, urls_dead
    );

    let pages_count = {
        let conn = db.content.lock().unwrap();
        crate::db::content::get_landing_page_count(&conn)?
    };
    println!("Landing Pages: {}", pages_count);

    let total_visits = {
        let conn = db.analytics.lock().unwrap();
        crate::db::analytics::get_total_clicks(&conn)?
    };
    println!("Redirect Clicks: {}", total_visits);

    Ok(())
}
