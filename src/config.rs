use std::path::PathBuf;
use std::env;
use std::fs;
use serde::Deserialize;

#[derive(Clone, Debug)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub data_dir: PathBuf,
    pub admin_username: String,
    pub bootstrap_password_sha256: String,
    pub session_secret: String,
    pub cookie_secure: bool,
    pub data_retention_days: Option<i64>,
    pub link_check_interval_mins: u64,
    pub aggregation_interval_mins: u64,
    pub backup_enabled: bool,
    pub backup_interval_mins: u64,
    pub backup_dir: PathBuf,
}

#[derive(Deserialize, Default)]
struct TomlConfig {
    host: Option<String>,
    port: Option<u16>,
    data_dir: Option<String>,
    admin_username: Option<String>,
    bootstrap_password_sha256: Option<String>,
    session_secret: Option<String>,
    cookie_secure: Option<bool>,
    data_retention_days: Option<String>,
    link_check_interval_mins: Option<u64>,
    aggregation_interval_mins: Option<u64>,
    backup: Option<TomlBackupConfig>,
}

#[derive(Deserialize, Default)]
struct TomlBackupConfig {
    enabled: Option<bool>,
    interval_mins: Option<u64>,
    out_dir: Option<String>,
}

impl Config {
    pub fn load() -> Self {
        // 1. Built-in defaults
        let mut host = "0.0.0.0".to_string();
        let mut port = 8080u16;
        let mut data_dir = PathBuf::from("./data");
        let mut admin_username = "admin".to_string();
        let mut bootstrap_password_sha256 = "".to_string();
        let mut session_secret = "bzod-default-session-secret-change-me-in-production-please-do-it".to_string();
        let mut cookie_secure = true;
        let mut data_retention_days = None;
        let mut link_check_interval_mins = 60u64;
        let mut aggregation_interval_mins = 60u64;
        let mut backup_enabled = false;
        let mut backup_interval_mins = 1440u64; // Default: once per day
        let mut backup_dir = PathBuf::from("./backups");

        // 2. Load bzod.toml if it exists
        let mut toml_path = "bzod.toml".to_string();
        if fs::metadata("bzod.toml").is_err() && fs::metadata("config/bzod.toml").is_ok() {
            toml_path = "config/bzod.toml".to_string();
        }
        if let Ok(toml_content) = fs::read_to_string(&toml_path) {
            if let Ok(toml_config) = toml::from_str::<TomlConfig>(&toml_content) {
                if let Some(h) = toml_config.host { host = h; }
                if let Some(p) = toml_config.port { port = p; }
                if let Some(d) = toml_config.data_dir { data_dir = PathBuf::from(d); }
                if let Some(u) = toml_config.admin_username { admin_username = u; }
                if let Some(s) = toml_config.bootstrap_password_sha256 { bootstrap_password_sha256 = s; }
                if let Some(sec) = toml_config.session_secret { session_secret = sec; }
                if let Some(c) = toml_config.cookie_secure { cookie_secure = c; }
                if let Some(ret) = toml_config.data_retention_days {
                    if ret.eq_ignore_ascii_case("unlimited") {
                        data_retention_days = None;
                    } else if let Ok(parsed) = ret.parse::<i64>() {
                        data_retention_days = Some(parsed);
                    }
                }
                if let Some(lc) = toml_config.link_check_interval_mins { link_check_interval_mins = lc; }
                if let Some(ag) = toml_config.aggregation_interval_mins { aggregation_interval_mins = ag; }
                if let Some(b) = toml_config.backup {
                    if let Some(be) = b.enabled { backup_enabled = be; }
                    if let Some(bi) = b.interval_mins { backup_interval_mins = bi; }
                    if let Some(bo) = b.out_dir { backup_dir = PathBuf::from(bo); }
                }
            }
        }

        // 3. Load .env if present
        let _ = dotenvy::dotenv();
        if fs::metadata("config/.env").is_ok() {
            let _ = dotenvy::from_path("config/.env");
        }

        // 4. Load from Environment Variables (taking highest precedence)
        if let Ok(h) = env::var("HOST") { host = h; }
        if let Ok(p_str) = env::var("PORT") {
            if let Ok(p) = p_str.parse::<u16>() { port = p; }
        }
        if let Ok(d_str) = env::var("DATA_DIR") { data_dir = PathBuf::from(d_str); }
        if let Ok(u) = env::var("ADMIN_USERNAME") { admin_username = u; }
        if let Ok(s) = env::var("BOOTSTRAP_PASSWORD_SHA256") { bootstrap_password_sha256 = s; }
        if let Ok(sec) = env::var("SESSION_SECRET") { session_secret = sec; }
        if let Ok(c_str) = env::var("COOKIE_SECURE") {
            if let Ok(c) = c_str.parse::<bool>() { cookie_secure = c; }
        }
        if let Ok(ret_str) = env::var("DATA_RETENTION_DAYS") {
            if ret_str.eq_ignore_ascii_case("unlimited") {
                data_retention_days = None;
            } else if let Ok(parsed) = ret_str.parse::<i64>() {
                data_retention_days = Some(parsed);
            }
        }
        if let Ok(lc_str) = env::var("LINK_CHECK_INTERVAL_MINS") {
            if let Ok(lc) = lc_str.parse::<u64>() { link_check_interval_mins = lc; }
        }
        if let Ok(ag_str) = env::var("AGGREGATION_INTERVAL_MINS") {
            if let Ok(ag) = ag_str.parse::<u64>() { aggregation_interval_mins = ag; }
        }
        if let Ok(be_str) = env::var("BACKUP_ENABLED") {
            if let Ok(be) = be_str.parse::<bool>() { backup_enabled = be; }
        }
        if let Ok(bi_str) = env::var("BACKUP_INTERVAL_MINS") {
            if let Ok(bi) = bi_str.parse::<u64>() { backup_interval_mins = bi; }
        }
        if let Ok(bo_str) = env::var("BACKUP_DIR") { backup_dir = PathBuf::from(bo_str); }

        Self {
            host,
            port,
            data_dir,
            admin_username,
            bootstrap_password_sha256,
            session_secret,
            cookie_secure,
            data_retention_days,
            link_check_interval_mins,
            aggregation_interval_mins,
            backup_enabled,
            backup_interval_mins,
            backup_dir,
        }
    }
}
