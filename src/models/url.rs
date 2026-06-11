use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Url {
    pub id: String,
    pub code: String,
    pub destination: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: String, // 'healthy', 'suspect', 'dead'
    pub created_at: String,
    pub updated_at: String,
    pub tags: Vec<String>,
}
