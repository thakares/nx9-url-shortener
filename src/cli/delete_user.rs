use crate::config::Config;
use crate::db::Db;
use chrono::Utc;
use std::path::PathBuf;
use tracing::{error, info};

pub async fn run(
    user_id: i64,
    force: bool,
    data_dir: Option<String>,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(d) = data_dir {
        config.data_dir = PathBuf::from(d);
    }
    let db = Db::init(&config)?;

    if user_id == 1 && !force {
        error!("Deleting legacy_admin system account requires --force flag");
        return Ok(());
    }

    // Capture user details
    let user_details = {
        let conn = db.users.lock().unwrap();
        match crate::db::users::get_user_by_id(&conn, user_id)? {
            Some(u) => u,
            None => {
                error!("User ID {} not found", user_id);
                return Ok(());
            }
        }
    };

    // 1. Transactional clean up on system.db (deleting their global slug mappings)
    {
        let mut system_conn = db.system.lock().unwrap();
        let tx = system_conn.transaction()?;

        // Get all slugs owned by the user
        let slugs: Vec<String> = {
            let mut stmt = tx.prepare("SELECT slug FROM global_slugs WHERE owner_user_id = ?1;")?;
            let rows = stmt.query_map([user_id], |row| row.get(0))?;
            rows.filter_map(|r| r.ok()).collect()
        };

        // Delete from global_slugs and write to history
        let now = Utc::now().to_rfc3339();
        for slug in slugs {
            let _ = tx.execute("DELETE FROM global_slugs WHERE slug = ?1;", [&slug]);
            let _ = tx.execute(
                "INSERT INTO slug_history (slug, old_owner_user_id, new_owner_user_id, action, timestamp, admin_username)
                 VALUES (?1, ?2, NULL, 'deleted', ?3, ?4);",
                rusqlite::params![slug, user_id, now, "cli"],
            );
        }

        tx.commit()?;
    }

    // 2. Delete user folder and database files from disk
    let user_dir = config.data_dir.join("users").join(user_id.to_string());
    if user_dir.exists() {
        let _ = std::fs::remove_dir_all(&user_dir);
    }

    // 3. Remove user entry from users.db (cascading deletes quotas/sessions/tokens)
    {
        let conn = db.users.lock().unwrap();
        crate::db::users::delete_user(&conn, user_id)?;
    }

    // Write audit event
    {
        let system_conn = db.system.lock().unwrap();
        let _ = crate::db::audit_events::write_audit_event(
            &system_conn,
            "cli",
            "USER_DELETION",
            "user",
            &user_id.to_string(),
            Some(&format!("Username: {}", user_details.username)),
        );
    }

    info!(
        "Successfully deleted user {} (ID: {}) and all associated content",
        user_details.username, user_id
    );

    Ok(())
}
