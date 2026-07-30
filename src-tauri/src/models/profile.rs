use serde::{Deserialize, Serialize};

/// A saved provider configuration (platform + model combination)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderProfile {
    pub id: String,
    /// Human-readable name (e.g. "OpenAI Work", "Claude Coding")
    pub name: String,
    /// Reference to Platform.id
    pub platform_id: String,
    /// Reference to Model.id
    pub model_id: String,
    /// Optional notes
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Unix timestamp of creation
    pub created_at: i64,
    /// Unix timestamp of last update
    pub updated_at: i64,
}
