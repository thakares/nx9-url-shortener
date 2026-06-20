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

    // 3. Register slug in system.db with status 'reserving' and check availability
    {
        let system_conn = db.system.lock().unwrap();
        if !crate::db::users::is_slug_available(&system_conn, &code)? {
            return Err("Short code/slug already exists".into());
        }
        crate::db::users::register_global_slug(&system_conn, &code, 1, "url", "", "reserving")?;
    }

    // 4. Persist URL
    let conn = db.content.lock().unwrap();
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
            // Activate slug in system.db
            {
                let system_conn = db.system.lock().unwrap();
                system_conn.execute(
                    "UPDATE global_slugs SET target_id = ?1, status = 'active', updated_at = ?2 WHERE slug = ?3;",
                    rusqlite::params![url.id, chrono::Utc::now().to_rfc3339(), code],
                )?;
            }
            // Increment quota for user ID 1
            {
                let users_conn = db.users.lock().unwrap();
                crate::db::users::increment_quota_counter(&users_conn, 1, "urls")?;
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
            let system_conn = db.system.lock().unwrap();
            let _ = crate::db::users::release_global_slug(&system_conn, &code, 1);
            Err(e.into())
        }
    }
}
