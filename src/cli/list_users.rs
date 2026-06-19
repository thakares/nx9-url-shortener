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

    let users = {
        let conn = db.users.lock().unwrap();
        crate::db::users::list_users(&conn)?
    };

    println!(
        "{:<6} | {:<20} | {:<10} | {:<12} | {:<24}",
        "ID", "Username", "Status", "Type", "Created At"
    );
    println!(
        "{:-<6}-+-{:-<20}-+-{:-<10}-+-{:-<12}-+-{:-<24}",
        "", "", "", "", ""
    );

    for u in users {
        println!(
            "{:<6} | {:<20} | {:<10} | {:<12} | {:<24}",
            u.id, u.username, u.status, u.account_type, u.created_at
        );
    }

    Ok(())
}
