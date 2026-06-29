use bzod::cli::{Cli, Commands};
use bzod::config::Config;
use clap::Parser;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set up tracing subscriber
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let config = Config::load();

    match cli.command {
        Commands::Serve {
            host,
            port,
            data_dir,
        } => {
            bzod::cli::serve::run(host, port, data_dir, config).await?;
        }
        Commands::Backup { out, data_dir } => {
            bzod::cli::backup::run(out, data_dir, config).await?;
        }
        Commands::Restore { file, data_dir } => {
            bzod::cli::restore::run(file, data_dir, config).await?;
        }
        Commands::Migrate { data_dir, dry_run } => {
            bzod::cli::migrate::run(data_dir, dry_run, config).await?;
        }
        Commands::Stats { data_dir } => {
            bzod::cli::stats::run(data_dir, config).await?;
        }
        Commands::Validate { data_dir } => {
            bzod::cli::validate::run(data_dir, config).await?;
        }
        Commands::CreateAdmin { username, data_dir } => {
            bzod::cli::create_admin::run(username, data_dir, config).await?;
        }
        Commands::Doctor { data_dir } => {
            bzod::cli::doctor::run(data_dir, config).await?;
        }
        Commands::Shorten {
            target_url,
            slug,
            data_dir,
        } => {
            bzod::cli::shorten::run(target_url, slug, data_dir, config).await?;
        }
        Commands::Expand { code, data_dir } => {
            bzod::cli::expand::run(code, data_dir, config).await?;
        }
        Commands::CreateUser {
            username,
            password,
            data_dir,
        } => {
            bzod::cli::create_user::run(username, password, data_dir, config).await?;
        }
        Commands::DeleteUser {
            user_id,
            force,
            data_dir,
        } => {
            bzod::cli::delete_user::run(user_id, force, data_dir, config).await?;
        }
        Commands::DisableUser { user_id, data_dir } => {
            bzod::cli::disable_user::run(user_id, data_dir, config).await?;
        }
        Commands::EnableUser { user_id, data_dir } => {
            bzod::cli::enable_user::run(user_id, data_dir, config).await?;
        }
        Commands::ResetPassword {
            user_id,
            password,
            data_dir,
        } => {
            bzod::cli::reset_password::run(user_id, password, data_dir, config).await?;
        }
        Commands::ListUsers { data_dir } => {
            bzod::cli::list_users::run(data_dir, config).await?;
        }
        Commands::BackupUser {
            username,
            out,
            data_dir,
        } => {
            bzod::cli::backup_user::run(username, out, data_dir, config).await?;
        }
        Commands::RestoreUser { file, data_dir } => {
            bzod::cli::restore_user::run(file, data_dir, config).await?;
        }
        Commands::AdminMigrate {
            target_admin_id,
            data_dir,
            dry_run,
            force,
        } => {
            bzod::cli::admin_migrate::run(target_admin_id, data_dir, dry_run, force, config)
                .await?;
        }
        Commands::Repair { command } => {
            bzod::cli::repair::run(command, config).await?;
        }
    }

    Ok(())
}
