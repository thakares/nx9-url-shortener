use std::path::PathBuf;
use crate::config::Config;
use crate::db::Db;

pub async fn run(
    data_dir: Option<String>,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(d) = data_dir { config.data_dir = PathBuf::from(d); }
    let db = Db::init(&config)?;
    
    println!("=== BZOD Database Stats ===");
    println!("Storage Directory: {:?}", config.data_dir);
    
    let files = vec!["admin.db", "content.db", "analytics.db", "system.db"];
    for f in files {
        let p = config.data_dir.join(f);
        if p.exists() {
            let sz = std::fs::metadata(&p)?.len();
            println!("  File: {} - Size: {} bytes ({:.2} MB)", f, sz, sz as f64 / 1_048_576.0);
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
    println!("Shortened URLs: {} total ({} active / {} dead)", urls_total, urls_active, urls_dead);

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
