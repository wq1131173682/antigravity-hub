use serde::{Deserialize, Serialize};

/// A saved Codex provider profile that remembers platform, model, and proxy settings.
/// Users can save multiple profiles and quickly switch between them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexProfile {
    /// Unique identifier (UUID v4)
    pub id: String,
    /// User-given name for this profile (e.g. "OpenAI Work", "DeepSeek Coding")
    pub name: String,
    /// The Antigravity Hub platform ID to route through
    pub platform_id: String,
    /// The model name to set in Codex CLI/Desktop
    pub model_name: String,
    /// Proxy host (from app config, usually 127.0.0.1)
    pub proxy_host: String,
    /// Proxy port (from app config, usually 8045)
    pub proxy_port: u16,
    /// Path prefix derived from the platform (e.g. "openai", "sensenova")
    pub path_prefix: String,
    /// Optional reasoning effort (low, medium, high)
    pub reasoning_effort: Option<String>,
    /// Whether to disable response storage in Codex CLI
    pub disable_response_storage: Option<bool>,
    /// Optional API key for the custom provider
    pub api_key: Option<String>,
    /// Creation timestamp (Unix epoch seconds)
    pub created_at: i64,
    /// Last update timestamp (Unix epoch seconds)
    pub updated_at: i64,
}
