use serde::{Deserialize, Serialize};

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
    // --- Feature Expansion Fields ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub expired: bool,
    #[serde(skip_serializing)]
    pub password_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_latency_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_access_count: Option<i64>,
    #[serde(default)]
    pub access_count: i64,
}

impl Url {
    /// Returns true if this URL has a password set.
    pub fn is_password_protected(&self) -> bool {
        self.password_hash.is_some()
    }

    /// Returns true if this URL has reached its access limit.
    pub fn is_access_exhausted(&self) -> bool {
        if let Some(max) = self.max_access_count {
            self.access_count >= max
        } else {
            false
        }
    }
}

/// Link preview metadata for smart landing pages.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LinkPreview {
    pub id: String,
    pub url_id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub logo_url: Option<String>,
    pub button_text: String,
}

/// QR code metadata record.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct QrCode {
    pub id: String,
    pub url_id: String,
    pub style: String,
    pub created_at: String,
}

/// Audit event record for the enhanced audit trail.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AuditEvent {
    pub id: String,
    pub actor: String,
    pub action: String,
    pub object_type: String,
    pub object_id: String,
    pub timestamp: String,
    pub metadata: Option<String>,
}
