use crate::auth::hash_password;
use crate::config::Config;
use crate::db::Db;
use std::io::{self, Write};
use std::path::PathBuf;
use tracing::{error, info};

pub async fn run(
    user_id: i64,
    password: Option<String>,
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

    let final_password = match password {
        Some(p) => p,
        None => read_input("Enter new password: "),
    };
    if final_password.trim().is_empty() {
        error!("Password cannot be empty");
        return Ok(());
    }

    let hash = hash_password(&final_password).map_err(|e| e.to_string())?;

    {
        let conn = db.users.lock().unwrap();
        crate::db::users::reset_user_password(&conn, user_id, &hash)?;
    }

    // Write audit event
    {
        let system_conn = db.system.lock().unwrap();
        let _ = crate::db::audit_events::write_audit_event(
            &system_conn,
            "cli",
            "USER_PASSWORD_RESET",
            "user",
            &user_id.to_string(),
            Some(&format!("Username: {}", user.username)),
        );
    }

    info!(
        "Password for user {} (ID: {}) has been reset successfully",
        user.username, user_id
    );
    Ok(())
}

fn read_input(prompt: &str) -> String {
    print!("{}", prompt);
    let _ = io::stdout().flush();
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
    input.trim().to_string()
}
