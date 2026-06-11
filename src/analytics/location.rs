use axum::http::HeaderMap;

// Guess or extract client country from headers
pub fn get_client_country(headers: &HeaderMap) -> String {
    if let Some(country) = headers.get("cf-ipcountry").and_then(|h| h.to_str().ok()) {
        return country.to_uppercase();
    }
    if let Some(lang) = headers.get("accept-language").and_then(|h| h.to_str().ok()) {
        if let Some(dash_idx) = lang.find('-') {
            if lang.len() > dash_idx + 2 {
                let code = &lang[dash_idx + 1..dash_idx + 3];
                if code.chars().all(|c| c.is_ascii_alphabetic()) {
                    return code.to_uppercase();
                }
            }
        }
    }
    "Unknown".to_string()
}
