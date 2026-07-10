use serde::{Deserialize, Serialize};

/// A path-specific base URL override.
/// When a request's target path starts with `path_prefix`, the proxy will
/// use `base_url` instead of the platform's default `base_url`.
/// This allows a single platform to serve endpoints at different API roots.
/// Example: for platform with base_url="https://api.example.com/v1",
/// you can add an override so that /agnesapi routes to
/// "https://api.example.com" (without /v1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathOverride {
    /// Path prefix to match (e.g. "/agnesapi")
    pub path_prefix: String,
    /// Alternative base URL to use (e.g. "https://apihub.agnes-ai.com")
    pub base_url: String,
}

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
    /// Path-specific base URL overrides (e.g., for endpoints at a different API root)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub base_url_overrides: Vec<PathOverride>,
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
            base_url_overrides: Vec::new(),
        }
    }
}
