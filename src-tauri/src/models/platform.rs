use serde::{Deserialize, Serialize};

/// A third-party API platform (e.g. OpenAI, Anthropic, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Platform {
    pub id: String,
    /// Display name (e.g. "OpenAI")
    pub name: String,
    /// Base URL without trailing slash (e.g. "https://api.openai.com")
    pub base_url: String,
    /// Path prefix used in the proxy to identify this platform (e.g. "openai")
    pub path_prefix: String,
    /// Optional notes
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Sort order
    #[serde(default)]
    pub sort_order: i32,
    /// Created at (unix timestamp)
    pub created_at: i64,
}

impl Platform {
    pub fn new(id: String, name: String, base_url: String, path_prefix: String) -> Self {
        Self {
            id,
            name,
            base_url,
            path_prefix,
            notes: None,
            sort_order: 0,
            created_at: chrono::Utc::now().timestamp(),
        }
    }
}
