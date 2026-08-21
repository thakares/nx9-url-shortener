use crate::auth::hash_password;
use crate::config::Config;
use crate::db::Db;
use std::env;
use std::path::PathBuf;
use tracing::{error, info};

pub async fn run(
    data_dir: Option<String>,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(d) = data_dir {
        config.data_dir = PathBuf::from(d);
    }
    let db = Db::init(&config)?;
    let admin_count: i64 = {
        let conn = db.users.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM users WHERE account_type = 'admin';",
            [],
            |row| row.get(0),
        )?
    };

    if admin_count > 0 {
        info!("Administrator already exists; initialization skipped.");
        return Ok(());
    }

    let username = match env::var("ADMIN_USERNAME") {
        Ok(u) if !u.trim().is_empty() => u.trim().to_string(),
        _ => {
            let msg = "No administrator exists.\nADMIN_USERNAME and ADMIN_PASSWORD are required for first-time initialization.";
            error!("{}", msg);
            return Err(msg.into());
        }
    };

    let password = match env::var("ADMIN_PASSWORD") {
        Ok(p) if !p.trim().is_empty() => p.trim().to_string(),
        _ => {
            let msg = "No administrator exists.\nADMIN_USERNAME and ADMIN_PASSWORD are required for first-time initialization.";
            error!("{}", msg);
            return Err(msg.into());
        }
    };

    let hash = hash_password(&password).map_err(|e| e.to_string())?;
    {
        let conn = db.users.lock().unwrap();
        let _ = crate::db::users::create_admin_user(&conn, &username, &hash)?;
    }
    info!("Administrator initialized successfully.");

    Ok(())
}
