use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LandingPage {
    pub id: String,
    pub code: String,
    pub slug: String,
    pub title: String,
    pub html_content: String,
    pub state: String, // 'draft', 'published', 'archived'
    pub created_at: String,
    pub updated_at: String,
}
