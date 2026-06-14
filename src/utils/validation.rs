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
