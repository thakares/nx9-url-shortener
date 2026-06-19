use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct User {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Session {
    pub id: String,
    pub user_id: String,
    pub expires_at: String,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TenantUser {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub status: String, // 'active', 'disabled', 'suspended', 'pending', 'deleted'
    pub created_at: String,
    pub last_login: Option<String>,
    pub account_type: String, // 'system', 'admin', 'standard', 'organization', 'service'
    pub organization_id: Option<i64>,
    pub metadata: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UserQuotas {
    pub user_id: i64,
    pub max_urls: i64,
    pub max_landings: i64,
    pub max_api_tokens: i64,
    pub max_storage_mb: i64,
    pub current_urls: i64,
    pub current_landings: i64,
    pub current_api_tokens: i64,
    pub current_storage_mb: i64,
}

impl UserQuotas {
    pub fn urls_pct(&self) -> f64 {
        if self.max_urls <= 0 {
            0.0
        } else {
            (self.current_urls as f64 / self.max_urls as f64 * 100.0).clamp(0.0, 100.0)
        }
    }
    pub fn landings_pct(&self) -> f64 {
        if self.max_landings <= 0 {
            0.0
        } else {
            (self.current_landings as f64 / self.max_landings as f64 * 100.0).clamp(0.0, 100.0)
        }
    }
    pub fn api_tokens_pct(&self) -> f64 {
        if self.max_api_tokens <= 0 {
            0.0
        } else {
            (self.current_api_tokens as f64 / self.max_api_tokens as f64 * 100.0).clamp(0.0, 100.0)
        }
    }
    pub fn storage_pct(&self) -> f64 {
        if self.max_storage_mb <= 0 {
            0.0
        } else {
            (self.current_storage_mb as f64 / self.max_storage_mb as f64 * 100.0).clamp(0.0, 100.0)
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UserApiToken {
    pub id: i64,
    pub user_id: i64,
    pub token_hash: String,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UserSession {
    pub id: String,
    pub user_id: i64,
    pub expires_at: String,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UsernameHistory {
    pub id: i64,
    pub user_id: i64,
    pub old_username: String,
    pub new_username: String,
    pub changed_at: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlugStatus {
    Active,
    Flagged,
    Disabled,
    SoftDeleted,
}

impl SlugStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Flagged => "flagged",
            Self::Disabled => "disabled",
            Self::SoftDeleted => "soft_deleted",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "active" => Some(Self::Active),
            "flagged" => Some(Self::Flagged),
            "disabled" => Some(Self::Disabled),
            "soft_deleted" => Some(Self::SoftDeleted),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountType {
    System,
    Admin,
    Standard,
    Organization,
    Service,
}

impl AccountType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Admin => "admin",
            Self::Standard => "standard",
            Self::Organization => "organization",
            Self::Service => "service",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "system" => Some(Self::System),
            "admin" => Some(Self::Admin),
            "standard" => Some(Self::Standard),
            "organization" => Some(Self::Organization),
            "service" => Some(Self::Service),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModerationSeverity {
    Low,
    Medium,
    High,
    Critical,
}

impl ModerationSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "critical" => Some(Self::Critical),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum ApiActor {
    Admin(User),
    User(TenantUser),
}

impl ApiActor {
    pub fn username(&self) -> &str {
        match self {
            Self::Admin(u) => &u.username,
            Self::User(u) => &u.username,
        }
    }
}
