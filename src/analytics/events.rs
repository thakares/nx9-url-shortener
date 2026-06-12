use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum AnalyticsEvent {
    RedirectVisit {
        url_id: String,
        code: String,
        timestamp: String,
        ip_address: String,
        user_agent: String,
        referer: String,
        accept_language: String,
        country: String,
        status_code: u16,
    },
    LandingPageVisit {
        page_id: String,
        code: String,
        slug: String,
        timestamp: String,
        ip_address: String,
        user_agent: String,
        referer: String,
        accept_language: String,
        country: String,
        status_code: u16,
    },
    AdminLogin {
        username: String,
        timestamp: String,
        ip_address: Option<String>,
        user_agent: Option<String>,
        success: bool,
    },
    ApiRequest {
        username: String,
        endpoint: String,
        method: String,
        timestamp: String,
        ip_address: Option<String>,
        status_code: u16,
    },
    SystemEvent {
        event_type: String, // 'startup', 'shutdown', 'backup', etc.
        details: String,
        timestamp: String,
    },
}
