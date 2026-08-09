pub fn validate_custom_slug(slug: &str) -> bool {
    if !slug.starts_with('!') {
        return false;
    }
    let rest = &slug[1..];
    if rest.is_empty() || rest.len() > 24 {
        return false;
    }
    rest.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

pub fn validate_redirect_code(code: &str) -> bool {
    (code.len() == 6 && code.chars().all(|c| c.is_ascii_hexdigit())) || validate_custom_slug(code)
}

pub fn validate_page_code(code: &str) -> bool {
    (code.len() == 4 && code.chars().all(|c| c.is_ascii_hexdigit())) || validate_custom_slug(code)
}

/// Classification of a stored or proposed redirect destination.
///
/// Used by write-path validation and read-only legacy data audits. Order of checks
/// matches `validate_redirect_destination` so both share the same rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestinationClass {
    ValidHttp,
    ValidHttps,
    Empty,
    TooLong,
    ControlCharacters,
    NonAscii,
    UnsupportedScheme,
    Malformed,
}

impl DestinationClass {
    pub fn is_valid(self) -> bool {
        matches!(self, Self::ValidHttp | Self::ValidHttps)
    }
}

fn scheme_prefix(dest: &str) -> Option<&str> {
    let end = dest.find(':')?;
    let scheme = &dest[..end];
    if scheme.is_empty() {
        return None;
    }
    if scheme
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '.' || c == '-')
    {
        Some(scheme)
    } else {
        None
    }
}

/// Classify a destination using the same rules as write-path validation.
pub fn classify_redirect_destination(destination: &str) -> DestinationClass {
    let dest = destination.trim();
    if dest.is_empty() {
        return DestinationClass::Empty;
    }
    if dest.len() > 2048 {
        return DestinationClass::TooLong;
    }
    // HTTP header / response-splitting: no CR, LF, NUL, or other ASCII controls.
    if dest.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return DestinationClass::ControlCharacters;
    }
    // HeaderValue also rejects non-visible ASCII in some cases; require pure ASCII.
    if !dest.is_ascii() {
        return DestinationClass::NonAscii;
    }
    // Reject non-http(s) schemes even when Url::parse fails (e.g. javascript:).
    if let Some(scheme) = scheme_prefix(dest) {
        let scheme_l = scheme.to_ascii_lowercase();
        if scheme_l != "http" && scheme_l != "https" {
            return DestinationClass::UnsupportedScheme;
        }
    }
    match reqwest::Url::parse(dest) {
        Ok(url) => {
            if url.host_str().is_none() {
                return DestinationClass::Malformed;
            }
            match url.scheme() {
                "http" => DestinationClass::ValidHttp,
                "https" => DestinationClass::ValidHttps,
                _ => DestinationClass::UnsupportedScheme,
            }
        }
        Err(_) => DestinationClass::Malformed,
    }
}

/// Returns true if `destination` is safe to store and emit as an HTTP Location value.
///
/// Rejects CR/LF and other ASCII control characters (response-splitting), empty values,
/// and non-http(s) schemes. The redirect handler remains defensive even if invalid
/// values already exist in older data.
pub fn validate_redirect_destination(destination: &str) -> bool {
    classify_redirect_destination(destination).is_valid()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_crlf_destination() {
        assert!(!validate_redirect_destination(
            "https://example.com/\r\nX-Injected: 1"
        ));
        assert!(!validate_redirect_destination(
            "https://example.com/\nX-Injected: 1"
        ));
    }

    #[test]
    fn rejects_non_http_schemes() {
        assert!(!validate_redirect_destination("javascript:alert(1)"));
        assert!(!validate_redirect_destination("data:text/html,hi"));
        assert!(!validate_redirect_destination("/relative/path"));
    }

    #[test]
    fn accepts_normal_https() {
        assert!(validate_redirect_destination(
            "https://example.com/path?q=1#frag"
        ));
        assert!(validate_redirect_destination("http://localhost:8080/x"));
    }

    #[test]
    fn classifies_destination_categories() {
        assert_eq!(
            classify_redirect_destination("https://ok.example/"),
            DestinationClass::ValidHttps
        );
        assert_eq!(
            classify_redirect_destination("http://ok.example/"),
            DestinationClass::ValidHttp
        );
        assert_eq!(
            classify_redirect_destination("javascript:alert(1)"),
            DestinationClass::UnsupportedScheme
        );
        assert_eq!(
            classify_redirect_destination("https://x/\r\nX:1"),
            DestinationClass::ControlCharacters
        );
        assert_eq!(
            classify_redirect_destination("not a url"),
            DestinationClass::Malformed
        );
        assert_eq!(classify_redirect_destination(""), DestinationClass::Empty);
    }
}
