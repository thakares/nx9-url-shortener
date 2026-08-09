//! Read-only audit of stored redirect destinations.
//!
//! Scans tenant content databases and classifies each `urls.destination` using
//! the same rules as write-path validation. Never rewrites or deletes data.

use crate::db::Db;
use crate::utils::validation::{classify_redirect_destination, DestinationClass};
use rusqlite::Connection;
use std::path::Path;
use tracing::{error, info, warn};

/// Summary counters for a destination audit run.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DestinationAuditReport {
    pub scanned_users: usize,
    pub total_urls: usize,
    pub valid_http: usize,
    pub valid_https: usize,
    pub invalid: usize,
    pub control_characters: usize,
    pub unsupported_scheme: usize,
    pub malformed: usize,
    pub empty: usize,
    pub too_long: usize,
    pub non_ascii: usize,
    /// Safe sample of invalid records: (owner_user_id, code, class_label).
    /// Destination bodies are never included (may contain control chars / secrets).
    pub invalid_samples: Vec<InvalidDestinationSample>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidDestinationSample {
    pub owner_user_id: i64,
    pub code: String,
    pub url_id: String,
    pub class: &'static str,
    pub destination_len: usize,
}

const MAX_SAMPLES: usize = 50;

fn class_label(c: DestinationClass) -> &'static str {
    match c {
        DestinationClass::ValidHttp => "valid_http",
        DestinationClass::ValidHttps => "valid_https",
        DestinationClass::Empty => "empty",
        DestinationClass::TooLong => "too_long",
        DestinationClass::ControlCharacters => "control_characters",
        DestinationClass::NonAscii => "non_ascii",
        DestinationClass::UnsupportedScheme => "unsupported_scheme",
        DestinationClass::Malformed => "malformed",
    }
}

/// Classify a single destination and update report counters.
pub fn record_destination(
    report: &mut DestinationAuditReport,
    owner_user_id: i64,
    code: &str,
    url_id: &str,
    destination: &str,
) {
    report.total_urls += 1;
    let class = classify_redirect_destination(destination);
    match class {
        DestinationClass::ValidHttp => report.valid_http += 1,
        DestinationClass::ValidHttps => report.valid_https += 1,
        DestinationClass::Empty => {
            report.empty += 1;
            report.invalid += 1;
        }
        DestinationClass::TooLong => {
            report.too_long += 1;
            report.invalid += 1;
        }
        DestinationClass::ControlCharacters => {
            report.control_characters += 1;
            report.invalid += 1;
        }
        DestinationClass::NonAscii => {
            report.non_ascii += 1;
            report.invalid += 1;
        }
        DestinationClass::UnsupportedScheme => {
            report.unsupported_scheme += 1;
            report.invalid += 1;
        }
        DestinationClass::Malformed => {
            report.malformed += 1;
            report.invalid += 1;
        }
    }

    if !class.is_valid() && report.invalid_samples.len() < MAX_SAMPLES {
        report.invalid_samples.push(InvalidDestinationSample {
            owner_user_id,
            code: code.to_string(),
            url_id: url_id.to_string(),
            class: class_label(class),
            destination_len: destination.len(),
        });
    }
}

/// Scan one content database connection for URL destinations.
pub fn audit_content_conn(
    conn: &Connection,
    owner_user_id: i64,
    report: &mut DestinationAuditReport,
) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare("SELECT id, code, destination FROM urls;")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    for row in rows {
        let (id, code, destination) = row?;
        record_destination(report, owner_user_id, &code, &id, &destination);
    }
    Ok(())
}

fn open_user_content(data_dir: &Path, user_id: i64) -> Result<Connection, rusqlite::Error> {
    let path = data_dir
        .join("users")
        .join(user_id.to_string())
        .join("content.db");
    if !path.exists() {
        return Err(rusqlite::Error::InvalidPath(path));
    }
    let conn = Connection::open(path)?;
    crate::db::sqlite::enable_wal(&conn, "content")?;
    Ok(conn)
}

/// Audit all tenant content databases found under the configured data directory.
///
/// Read-only: does not modify any records.
pub fn audit_all_destinations(db: &Db) -> Result<DestinationAuditReport, String> {
    let mut report = DestinationAuditReport::default();

    let user_ids: Vec<i64> = {
        let users = db
            .users
            .lock()
            .map_err(|e| format!("users_db lock poisoned: {}", e))?;
        let mut stmt = users
            .prepare("SELECT id FROM users;")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        rows.filter_map(|r| r.ok()).collect()
    };

    for user_id in user_ids {
        match open_user_content(&db.data_dir, user_id) {
            Ok(conn) => {
                report.scanned_users += 1;
                if let Err(e) = audit_content_conn(&conn, user_id, &mut report) {
                    error!(
                        owner_user_id = user_id,
                        error = %e,
                        "destination audit failed for user content.db"
                    );
                    return Err(format!("audit user {} content.db: {}", user_id, e));
                }
            }
            Err(rusqlite::Error::InvalidPath(_)) => {
                // User has no content DB yet — skip.
            }
            Err(e) => {
                warn!(
                    owner_user_id = user_id,
                    error = %e,
                    "could not open user content.db for destination audit"
                );
            }
        }
    }

    info!(
        total_urls = report.total_urls,
        valid = report.valid_http + report.valid_https,
        invalid = report.invalid,
        "destination audit complete"
    );
    Ok(report)
}

/// Format a human-readable report for CLI output.
pub fn format_report(report: &DestinationAuditReport) -> String {
    let mut out = String::new();
    out.push_str("BZOD Redirect Destination Audit (read-only)\n");
    out.push_str("===========================================\n");
    out.push_str(&format!("Users scanned:        {}\n", report.scanned_users));
    out.push_str(&format!("Total URLs:           {}\n", report.total_urls));
    out.push_str(&format!("Valid HTTP:           {}\n", report.valid_http));
    out.push_str(&format!("Valid HTTPS:          {}\n", report.valid_https));
    out.push_str(&format!("Invalid (total):      {}\n", report.invalid));
    out.push_str(&format!(
        "  control characters: {}\n",
        report.control_characters
    ));
    out.push_str(&format!(
        "  unsupported scheme: {}\n",
        report.unsupported_scheme
    ));
    out.push_str(&format!("  malformed:          {}\n", report.malformed));
    out.push_str(&format!("  empty:              {}\n", report.empty));
    out.push_str(&format!("  too long:           {}\n", report.too_long));
    out.push_str(&format!("  non-ascii:          {}\n", report.non_ascii));

    if !report.invalid_samples.is_empty() {
        out.push_str("\nInvalid samples (id/code only; destinations not printed):\n");
        for s in &report.invalid_samples {
            out.push_str(&format!(
                "  user={} code={} id={} class={} dest_len={}\n",
                s.owner_user_id, s.code, s.url_id, s.class, s.destination_len
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_control_character_destination() {
        let mut report = DestinationAuditReport::default();
        record_destination(
            &mut report,
            1,
            "ab12cd",
            "id-1",
            "https://evil.example/\r\nX:1",
        );
        assert_eq!(report.total_urls, 1);
        assert_eq!(report.invalid, 1);
        assert_eq!(report.control_characters, 1);
        assert_eq!(report.invalid_samples.len(), 1);
        assert_eq!(report.invalid_samples[0].class, "control_characters");
        // Ensure we never store the destination body in the sample.
        assert!(!format!("{:?}", report.invalid_samples[0]).contains("evil"));
    }

    #[test]
    fn records_valid_https() {
        let mut report = DestinationAuditReport::default();
        record_destination(&mut report, 1, "ab12cd", "id-1", "https://example.com/ok");
        assert_eq!(report.valid_https, 1);
        assert_eq!(report.invalid, 0);
        assert!(report.invalid_samples.is_empty());
    }
}
