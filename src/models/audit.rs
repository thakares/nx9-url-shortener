use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AuditLog {
    pub id: String,
    pub timestamp: String,
    pub username: String,
    pub action: String,
    pub object_type: Option<String>,
    pub object_id: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}
