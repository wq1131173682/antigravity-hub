use serde::{Deserialize, Serialize};

/// A model under a platform, with its own quota limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    /// Platform this model belongs to
    pub platform_id: String,
    /// Model identifier used in API requests (e.g. "gpt-4")
    pub model_name: String,
    /// Human-readable display name (e.g. "GPT-4")
    pub display_name: String,
    /// Max requests per 5-hour window
    #[serde(default = "default_5hour")]
    pub per_5hour: u32,
    /// Max requests per day
    #[serde(default = "default_day")]
    pub per_day: u32,
    /// Max requests per month
    #[serde(default = "default_month")]
    pub per_month: u32,
    /// Sort order
    #[serde(default)]
    pub sort_order: i32,
    /// Created at (unix timestamp)
    pub created_at: i64,
}

fn default_5hour() -> u32 { 3000 }
fn default_day() -> u32 { 10000 }
fn default_month() -> u32 { 100000 }

impl Model {
    pub fn new(platform_id: String, model_name: String, display_name: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            platform_id,
            model_name,
            display_name,
            per_5hour: default_5hour(),
            per_day: default_day(),
            per_month: default_month(),
            sort_order: 0,
            created_at: chrono::Utc::now().timestamp(),
        }
    }
}
