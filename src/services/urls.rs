//! Shared URL write-path helpers used by admin UI, tenant UI, and REST API.
//!
//! Handlers remain responsible for auth/CSRF/quotas; this module owns pure
//! destination preparation that must stay consistent across entry points.

use crate::utils::validation::validate_redirect_destination;

/// Optional UTM parameters applied to a destination URL.
#[derive(Debug, Default, Clone)]
pub struct UtmParams<'a> {
    pub source: Option<&'a str>,
    pub medium: Option<&'a str>,
    pub campaign: Option<&'a str>,
}

/// Normalize and optionally append UTM parameters to a destination.
///
/// Returns `Err` when the base destination fails canonical validation.
/// UTM appending only runs when the base parses as a URL (same as prior handlers).
pub fn prepare_destination(raw: &str, utm: UtmParams<'_>) -> Result<String, &'static str> {
    let mut dest = raw.trim().to_string();
    if !validate_redirect_destination(&dest) {
        return Err("Destination must be a valid http(s) URL without control characters");
    }

    if let Ok(mut parsed) = reqwest::Url::parse(&dest) {
        let mut has_utm = false;
        {
            let mut query = parsed.query_pairs_mut();
            if let Some(src) = utm.source {
                let src = src.trim();
                if !src.is_empty() {
                    query.append_pair("utm_source", src);
                    has_utm = true;
                }
            }
            if let Some(med) = utm.medium {
                let med = med.trim();
                if !med.is_empty() {
                    query.append_pair("utm_medium", med);
                    has_utm = true;
                }
            }
            if let Some(camp) = utm.campaign {
                let camp = camp.trim();
                if !camp.is_empty() {
                    query.append_pair("utm_campaign", camp);
                    has_utm = true;
                }
            }
        }
        if has_utm {
            dest = parsed.to_string();
        }
    }

    Ok(dest)
}

/// Parse HTML datetime-local / partial RFC3339 expiry input into RFC3339 if present.
pub fn parse_expires_at_input(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut rfc = trimmed.to_string();
    if rfc.len() == 16 {
        // HTML datetime-local → assume UTC seconds
        rfc.push_str(":00Z");
    }
    Some(rfc)
}

/// Parse optional max-access-count form field.
pub fn parse_max_access_count(raw: &str) -> Option<i64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_rejects_crlf() {
        let err = prepare_destination("https://x/\r\nY:1", UtmParams::default()).unwrap_err();
        assert!(err.contains("valid http"));
    }

    #[test]
    fn prepare_appends_utm() {
        let dest = prepare_destination(
            "https://example.com/path",
            UtmParams {
                source: Some("newsletter"),
                medium: Some("email"),
                campaign: Some("spring"),
            },
        )
        .unwrap();
        assert!(dest.contains("utm_source=newsletter"));
        assert!(dest.contains("utm_medium=email"));
        assert!(dest.contains("utm_campaign=spring"));
    }

    #[test]
    fn parse_expires_datetime_local() {
        assert_eq!(
            parse_expires_at_input("2030-01-01T12:00"),
            Some("2030-01-01T12:00:00Z".to_string())
        );
        assert_eq!(parse_expires_at_input("  "), None);
    }
}
