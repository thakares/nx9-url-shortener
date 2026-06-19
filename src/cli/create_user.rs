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
        None => read_input("Enter username: "),
    };

    let username_clean = final_username.trim().to_lowercase();
    if username_clean.is_empty() {
        error!("Username cannot be empty");
        return Ok(());
    }

    if username_clean.len() < 3 {
        error!("Username must be at least 3 characters");
        return Ok(());
    }

    if !username_clean
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        error!("Username must contain only alphanumeric characters, hyphens, or underscores");
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

    // Check if user already exists
    {
        let conn = db.users.lock().unwrap();
        if crate::db::users::get_user_by_username(&conn, &username_clean)?.is_some() {
            error!("User already exists: {}", username_clean);
            return Ok(());
        }
    }

    // Create user in DB (this seeds default quotas too)
    let new_user = {
        let conn = db.users.lock().unwrap();
        crate::db::users::create_user(&conn, &username_clean, &hash, "standard", None)?
    };

    // Initialize their user specific directory and DB files (content.db, analytics.db, profile.db)
    db.init_user_databases(new_user.id)?;

    info!(
        "Successfully created standard user: {} (ID: {})",
        new_user.username, new_user.id
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
