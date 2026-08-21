use crate::config::Config;
use crate::db::Db;
use std::path::PathBuf;

pub async fn run(
    target_url: String,
    slug: Option<String>,
    data_dir: Option<String>,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Basic URL validation
    if reqwest::Url::parse(&target_url).is_err() {
        return Err("Invalid destination URL format".into());
    }

    if let Some(d) = data_dir {
        config.data_dir = PathBuf::from(d);
    }
    let db = Db::init(&config)?;

    // Resolve active standard user tenant
    let (user_id, tid) = {
        let users_conn = db.users.lock().unwrap();
        let users = crate::db::users::list_users(&users_conn)?;
        let active_user = users.into_iter().find(|u| {
            u.account_type == "standard" && u.status == "active" && u.tenant_id.is_some()
        });
        match active_user {
            Some(u) => (u.id, u.tenant_id.unwrap()),
            None => {
                return Err(
                    "Cannot shorten URL: No active standard user tenant found. Create a standard user first with `bzod user create`."
                        .into(),
                )
            }
        }
    };

    // 2. Validate/normalize slug/code
    let code = match slug {
        Some(s) => {
            let normalized = s.trim().to_lowercase();
            if !crate::utils::validation::validate_custom_slug(&normalized) {
                return Err(
                    "Custom slug must start with ! followed by 1-24 characters of a-z, 0-9, -, _"
                        .into(),
                );
            }
            normalized
        }
        None => crate::utils::random::generate_token(3),
    };

    // 3. Register slug in global_urls with status 'reserving'
    {
        let urls_conn = db.global_urls.lock().unwrap();
        let pages_conn = db.global_landing_pages.lock().unwrap();
        let reserved_conn = db.reserved.lock().unwrap();
        if let Err(e) =
            crate::db::slugs::reserve_url_slug(&reserved_conn, &urls_conn, &pages_conn, &code, &tid)
        {
            return Err(format!("Slug '{}' already exists or is unavailable: {}", code, e).into());
        }
    }

    // 4. Persist URL in tenant content DB
    let content_path = db.topology.tenant_content_db(tid);
    let conn = rusqlite::Connection::open(&content_path)?;
    let _ = crate::db::sqlite::enable_wal(&conn, "content");
    let _ = crate::db::sqlite::enable_foreign_keys(&conn, "content");

    let res = crate::db::content::create_url_extended(
        &conn,
        &code,
        &target_url,
        None,
        None,
        &[],
        None,
        None,
        None,
    );

    match res {
        Ok(url) => {
            // Activate slug in global_urls
            {
                let urls_conn = db.global_urls.lock().unwrap();
                crate::db::slugs::activate_url_slug(&urls_conn, &code, &url.id)?;
            }
            // Increment quota
            {
                let users_conn = db.users.lock().unwrap();
                crate::db::users::increment_quota_counter(&users_conn, user_id, "urls")?;
            }

            let proto = if config.cookie_secure {
                "https"
            } else {
                "http"
            };
            let base_url = config
                .base_url
                .clone()
                .unwrap_or_else(|| format!("{}://localhost:{}", proto, config.port));

            // Output only the shortened URL as requested
            println!("{}/{}", base_url, code);
            Ok(())
        }
        Err(e) => {
            let urls_conn = db.global_urls.lock().unwrap();
            let _ = crate::db::slugs::release_url_slug(&urls_conn, &code, &tid);
            Err(e.into())
        }
    }
}
