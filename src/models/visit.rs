use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VisitRecord {
    pub id: String,
    pub target_type: String, // 'url' or 'page'
    pub target_id: String,
    pub timestamp: String,
    pub ip_address: String,
    pub user_agent: String,
    pub referer: String,
    pub accept_language: String,
    pub country: String,
    pub status_code: u16,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SummaryEntry {
    pub date: String,
    pub target_type: String,
    pub target_id: String,
    pub metric_type: String,
    pub metric_key: String,
    pub metric_value: i64,
}
