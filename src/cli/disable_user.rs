use crate::config::Config;
use crate::db::Db;
use std::path::PathBuf;
use tracing::{error, info};

pub async fn run(
    user_id: i64,
    data_dir: Option<String>,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(d) = data_dir {
        config.data_dir = PathBuf::from(d);
    }
    let db = Db::init(&config)?;

    // Check user exists
    let user = {
        let conn = db.users.lock().unwrap();
        crate::db::users::get_user_by_id(&conn, user_id)?
    };

    let user = match user {
        Some(u) => u,
        None => {
            error!("User ID {} not found", user_id);
            return Ok(());
        }
    };

    if user.status == "disabled" {
        info!("User {} is already disabled", user.username);
        return Ok(());
    }

    {
        let conn = db.users.lock().unwrap();
        crate::db::users::update_user_status(&conn, user_id, "disabled")?;
    }

    // Write audit event
    {
        let system_conn = db.system.lock().unwrap();
        let _ = crate::db::audit_events::write_audit_event(
            &system_conn,
            "cli",
            "USER_DISABLED",
            "user",
            &user_id.to_string(),
            Some(&format!("Username: {}", user.username)),
        );
    }

    info!("User {} (ID: {}) has been disabled", user.username, user_id);
    Ok(())
}
