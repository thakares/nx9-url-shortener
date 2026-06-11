use axum::{
    response::{IntoResponse, Response},
    http::StatusCode,
};
use std::fmt;
use std::path::PathBuf;

/// Structured error type for database initialization and migration failures.
///
/// Each variant provides actionable context about what went wrong during startup,
/// making diagnosis possible from logs alone without needing a debugger.
#[derive(Debug)]
pub enum DatabaseInitError {
    /// Data directory could not be created or accessed
    DataDirCreate { path: PathBuf, source: std::io::Error },
    /// SQLite connection could not be opened
    ConnectionOpen { database: String, path: PathBuf, source: rusqlite::Error },
    /// PRAGMA configuration failed (WAL, foreign_keys, etc.)
    PragmaConfig { database: String, pragma: String, source: rusqlite::Error },
    /// Migration execution failed
    MigrationFailed { database: String, version: u32, name: String, source: Box<dyn std::error::Error + Send + Sync> },
    /// Database integrity check failed
    IntegrityCheckFailed { database: String, message: String },
    /// WAL mode could not be enabled (returned unexpected mode)
    WalModeFailed { database: String, actual_mode: String },
}

impl fmt::Display for DatabaseInitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DataDirCreate { path, source } => {
                write!(f, "Failed to create data directory {:?}: {}", path, source)
            }
            Self::ConnectionOpen { database, path, source } => {
                write!(f, "Failed to open {}.db at {:?}: {}", database, path, source)
            }
            Self::PragmaConfig { database, pragma, source } => {
                write!(f, "PRAGMA {} failed on {}.db: {}", pragma, database, source)
            }
            Self::MigrationFailed { database, version, name, source } => {
                write!(f, "Migration v{} ({}) failed on {}.db: {}", version, name, database, source)
            }
            Self::IntegrityCheckFailed { database, message } => {
                write!(f, "Integrity check failed on {}.db: {}", database, message)
            }
            Self::WalModeFailed { database, actual_mode } => {
                write!(f, "WAL mode not enabled on {}.db (got '{}')", database, actual_mode)
            }
        }
    }
}

impl std::error::Error for DatabaseInitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::DataDirCreate { source, .. } => Some(source),
            Self::ConnectionOpen { source, .. } => Some(source),
            Self::PragmaConfig { source, .. } => Some(source),
            Self::MigrationFailed { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum AppError {
    Db(rusqlite::Error),
    Auth(String),
    Template(askama::Error),
    Json(serde_json::Error),
    Io(std::io::Error),
    Http(String),
    NotFound(String),
    BadRequest(String),
    Unauthorized(String),
    Internal(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Db(e) => write!(f, "Database error: {}", e),
            AppError::Auth(e) => write!(f, "Authentication error: {}", e),
            AppError::Template(e) => write!(f, "Template render error: {}", e),
            AppError::Json(e) => write!(f, "JSON processing error: {}", e),
            AppError::Io(e) => write!(f, "IO error: {}", e),
            AppError::Http(e) => write!(f, "HTTP request error: {}", e),
            AppError::NotFound(e) => write!(f, "Not found: {}", e),
            AppError::BadRequest(e) => write!(f, "Bad request: {}", e),
            AppError::Unauthorized(e) => write!(f, "Unauthorized: {}", e),
            AppError::Internal(e) => write!(f, "Internal error: {}", e),
        }
    }
}

impl std::error::Error for AppError {}

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        AppError::Db(err)
    }
}

impl From<askama::Error> for AppError {
    fn from(err: askama::Error) -> Self {
        AppError::Template(err)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::Json(err)
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Io(err)
    }
}

impl From<argon2::password_hash::Error> for AppError {
    fn from(err: argon2::password_hash::Error) -> Self {
        AppError::Auth(err.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(err: reqwest::Error) -> Self {
        AppError::Http(err.to_string())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            AppError::Auth(_) => StatusCode::UNAUTHORIZED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };

        let message = self.to_string();
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!("AppError encountered: {:?}", self);
        }

        // Return error details as text. Handlers requiring custom UI/JSON can catch errors
        // or map them prior to calling into_response.
        (status, message).into_response()
    }
}
