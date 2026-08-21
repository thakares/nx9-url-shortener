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

    if user_details.account_type == "admin" && !force {
        error!("Deleting administrator account requires --force flag");
        return Ok(());
    }

    // 1. Retire/cleanup slugs from v0.8 global_urls.db and global_landing_pages.db
    let now = Utc::now().to_rfc3339();
    if let Some(ref tid) = user_details.tenant_id {
        let tid_str = tid.to_string();
        let urls_conn = db.global_urls.lock().unwrap();
        let pages_conn = db.global_landing_pages.lock().unwrap();
        let system_conn = db.system.lock().unwrap();

        // Get all URL slugs owned by tenant
        let mut stmt =
            urls_conn.prepare("SELECT slug FROM global_urls WHERE owner_tenant_id = ?1;")?;
        let url_slugs: Vec<String> = stmt
            .query_map([&tid_str], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        // Get all page slugs owned by tenant
        let mut stmt2 = pages_conn
            .prepare("SELECT slug FROM global_landing_pages WHERE owner_tenant_id = ?1;")?;
        let page_slugs: Vec<String> = stmt2
            .query_map([&tid_str], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        // Record history and retire/delete slugs
        for slug in url_slugs {
            let _ = urls_conn.execute("DELETE FROM global_urls WHERE slug = ?1;", [&slug]);
            let _ = system_conn.execute(
                "INSERT INTO slug_history (slug, old_owner_user_id, new_owner_user_id, action, timestamp, admin_username)
                 VALUES (?1, ?2, NULL, 'deleted', ?3, ?4);",
                rusqlite::params![slug, user_id, now, "cli"],
            );
        }

        for slug in page_slugs {
            let _ =
                pages_conn.execute("DELETE FROM global_landing_pages WHERE slug = ?1;", [&slug]);
            let _ = system_conn.execute(
                "INSERT INTO slug_history (slug, old_owner_user_id, new_owner_user_id, action, timestamp, admin_username)
                 VALUES (?1, ?2, NULL, 'deleted', ?3, ?4);",
                rusqlite::params![slug, user_id, now, "cli"],
            );
        }
    }

    // 2. Delete tenant directory from disk
    if let Some(ref tid) = user_details.tenant_id {
        let user_dir = db.topology.tenant_dir(*tid);
        if user_dir.exists() {
            let _ = std::fs::remove_dir_all(&user_dir);
        }
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
