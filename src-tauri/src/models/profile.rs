use serde::{Deserialize, Serialize};

/// Rotation strategy for failover between provider profiles
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RotationStrategy {
    /// No rotation — use only this profile
    None,
    /// Failover — try profiles in priority order
    Failover,
}

impl Default for RotationStrategy {
    fn default() -> Self {
        Self::Failover
    }
}

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
    /// Whether this profile is currently active (used for routing)
    #[serde(default)]
    pub active: bool,
    /// Priority order for failover rotation (lower = higher priority)
    #[serde(default)]
    pub priority: i32,
    /// Rotation strategy for this profile
    #[serde(default)]
    pub rotation_strategy: RotationStrategy,
    /// Unix timestamp of creation
    pub created_at: i64,
    /// Unix timestamp of last update
    pub updated_at: i64,
}
