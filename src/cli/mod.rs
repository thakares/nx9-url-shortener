use clap::{Parser, Subcommand};

pub mod backup;
pub mod create_admin;
pub mod doctor;
pub mod expand;
pub mod migrate;
pub mod restore;
pub mod serve;
pub mod shorten;
pub mod stats;
pub mod validate;

#[derive(Parser)]
#[command(name = "bzod")]
#[command(about = "BZOD - Personal Redirector & Landing Page Platform")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the BZOD web server
    Serve {
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        data_dir: Option<String>,
    },
    /// Create a tar.gz backup of all databases
    Backup {
        #[arg(long)]
        out: Option<String>,
        #[arg(long)]
        data_dir: Option<String>,
    },
    /// Restore databases from a tar.gz backup file
    Restore {
        #[arg(long, required = true)]
        file: String,
        #[arg(long)]
        data_dir: Option<String>,
    },
    /// Apply pending database schema migrations
    Migrate {
        #[arg(long)]
        data_dir: Option<String>,
        /// Show what migrations would be applied without executing them
        #[arg(long)]
        dry_run: bool,
    },
    /// Print database statistics and record counts in the terminal
    Stats {
        #[arg(long)]
        data_dir: Option<String>,
    },
    /// Perform a one-shot validation of all registered short link destinations
    Validate {
        #[arg(long)]
        data_dir: Option<String>,
    },
    /// Create a new administrator user in the database
    CreateAdmin {
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        data_dir: Option<String>,
    },
    /// Run database diagnostics and health checks
    Doctor {
        #[arg(long)]
        data_dir: Option<String>,
    },
    /// Shorten a URL (Feature 3)
    Shorten {
        /// The destination URL to shorten
        target_url: String,
        /// Custom slug (starting with ! followed by a-z, 0-9, -, _)
        #[arg(long)]
        slug: Option<String>,
        #[arg(long)]
        data_dir: Option<String>,
    },
    /// Expand a shortened code or custom slug to its destination URL (Feature 4)
    Expand {
        /// The short code or custom slug to expand
        code: String,
        #[arg(long)]
        data_dir: Option<String>,
    },
}
