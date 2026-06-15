use serde::{Deserialize, Serialize};

/// Status of an API key
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum KeyStatus {
    /// Key is active and ready for use
    #[serde(rename = "active")]
    Active,
    /// Key is temporarily disabled (rate limited or server error)
    #[serde(rename = "disabled")]
    Disabled,
}

impl Default for KeyStatus {
    fn default() -> Self {
        Self::Active
    }
}

/// An API key for a platform
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: String,
    /// Platform this key belongs to
    pub platform_id: String,
    /// Display name (e.g. "Key 1", "Team key")
    pub name: String,
    /// The actual API key value (masked in UI)
    pub key_value: String,
    /// Current status
    #[serde(default)]
    pub status: KeyStatus,
    /// Why the key was disabled
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<String>,
    /// Until when the key is disabled (unix timestamp)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_until: Option<i64>,
    /// Sort order
    #[serde(default)]
    pub sort_order: i32,
    /// Created at (unix timestamp)
    pub created_at: i64,
}

impl ApiKey {
    pub fn new(id: String, platform_id: String, name: String, key_value: String) -> Self {
        Self {
            id,
            platform_id,
            name,
            key_value,
            status: KeyStatus::Active,
            disabled_reason: None,
            disabled_until: None,
            sort_order: 0,
            created_at: chrono::Utc::now().timestamp(),
        }
    }

    pub fn is_active(&self) -> bool {
        if self.status == KeyStatus::Disabled {
            // Check if temporary disable has expired
            if let Some(until) = self.disabled_until {
                let now = chrono::Utc::now().timestamp();
                if now < until {
                    return false;
                }
            } else {
                // Permanently disabled
                return false;
            }
        }
        true
    }
}
