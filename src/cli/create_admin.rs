use std::path::PathBuf;
use std::io::{self, Write};
use tracing::{info, error};
use crate::config::Config;
use crate::db::Db;
use crate::auth::hash_password;

pub async fn run(
    username: Option<String>,
    data_dir: Option<String>,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(d) = data_dir { config.data_dir = PathBuf::from(d); }
    let db = Db::init(&config)?;

    let final_username = match username {
        Some(u) => u,
        None => read_input("Enter administrator username: "),
    };

    if final_username.trim().is_empty() {
        error!("Username cannot be empty");
        return Ok(());
    }

    let password = read_input("Enter password: ");
    if password.trim().is_empty() {
        error!("Password cannot be empty");
        return Ok(());
    }

    let hash = hash_password(&password).map_err(|e| e.to_string())?;
    let conn = db.admin.lock().unwrap();
    let u = crate::db::admin::create_user(&conn, &final_username, &hash)?;
    info!("Successfully created admin user: {} (ID: {})", u.username, u.id);

    Ok(())
}

fn read_input(prompt: &str) -> String {
    print!("{}", prompt);
    let _ = io::stdout().flush();
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
    input.trim().to_string()
}
