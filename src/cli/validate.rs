use crate::config::Config;
use crate::db::Db;
use reqwest::Client;
use std::path::PathBuf;
use std::time::Duration;
use tracing::info;

pub async fn run(
    data_dir: Option<String>,
    mut config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(d) = data_dir {
        config.data_dir = PathBuf::from(d);
    }

    let db = Db::init(&config)?;

    info!("Running one-shot link validation...");
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("bzod-cli-checker/0.1")
        .build()?;

    crate::jobs::perform_link_check(&db, &client).await?;
    info!("Link validation complete.");

    Ok(())
}
