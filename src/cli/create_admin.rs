use crate::auth::hash_password;
use crate::config::Config;
use crate::db::Db;
use std::io::{self, Write};
use std::path::PathBuf;
use tracing::{error, info};

pub async fn run(
    username: Option<String>,
    password: Option<String>,
    data_dir: Option<String>,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(d) = data_dir {
        config.data_dir = PathBuf::from(d);
    }
    let db = Db::init(&config)?;

    let final_username = match username {
        Some(u) => u,
        None => read_input("Enter administrator username: "),
    };

    if final_username.trim().is_empty() {
        error!("Username cannot be empty");
        return Ok(());
    }

    let final_password = match password {
        Some(p) => p,
        None => read_input("Enter password: "),
    };
    if final_password.trim().is_empty() {
        error!("Password cannot be empty");
        return Ok(());
    }

    let hash = hash_password(&final_password).map_err(|e| e.to_string())?;
    let conn = db.users.lock().unwrap();
    let u = crate::db::users::create_admin_user(&conn, &final_username, &hash)?;
    info!(
        "Successfully created admin user: {} (ID: {})",
        u.username, u.id
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
