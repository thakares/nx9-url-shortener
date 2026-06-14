use crate::config::Config;
use crate::db::Db;
use std::path::PathBuf;

pub async fn run(
    code: String,
    data_dir: Option<String>,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(d) = data_dir {
        config.data_dir = PathBuf::from(d);
    }
    let db = Db::init(&config)?;

    let normalized_code = code.trim().to_lowercase();
    if !crate::utils::validation::validate_redirect_code(&normalized_code) {
        return Err("Invalid short code or custom slug format".into());
    }

    let url_opt = {
        let conn = db.content.lock().unwrap();
        crate::db::content::get_url_by_code(&conn, &normalized_code)?
    };

    match url_opt {
        Some(url) => {
            println!("{}", url.destination);
            Ok(())
        }
        None => Err(format!("Short code not found: {}", normalized_code).into()),
    }
}
