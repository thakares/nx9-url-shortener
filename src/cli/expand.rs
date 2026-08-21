use crate::config::Config;
use crate::db::Db;
use std::path::PathBuf;

pub async fn run(
    code: String,
    data_dir: Option<String>,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(d) = data_dir {
        config.data_dir = PathBuf::from(d);
    }
    let db = Db::init(&config)?;

    let normalized_code = code.trim().to_lowercase();
    if !crate::utils::validation::validate_redirect_code(&normalized_code) {
        return Err("Invalid short code or custom slug format".into());
    }

    let owner_info = {
        let conn = db.global_urls.lock().unwrap();
        conn.query_row(
            "SELECT owner_tenant_id FROM global_urls WHERE slug = ?1 AND status = 'active';",
            rusqlite::params![normalized_code],
            |row| row.get::<_, String>(0),
        )
        .ok()
    };

    let tid_str = match owner_info {
        Some(t) => t,
        None => return Err(format!("Short code not found: {}", normalized_code).into()),
    };
    let tid = tid_str
        .parse::<crate::identity::TenantId>()
        .map_err(|e| format!("Invalid tenant id for slug {}: {:?}", normalized_code, e))?;

    let content_path = db.topology.tenant_content_db(tid);
    let conn = rusqlite::Connection::open(&content_path)?;
    let url_opt = crate::db::content::get_url_by_code(&conn, &normalized_code)?;

    match url_opt {
        Some(url) => {
            println!("{}", url.destination);
            Ok(())
        }
        None => Err(format!("Short code not found: {}", normalized_code).into()),
    }
}
