use serde::{Deserialize, Serialize};
use super::platform::Platform;

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub language: String,
    pub theme: String,
    /// Local proxy port
    #[serde(default = "default_proxy_port")]
    pub proxy_port: u16,
    /// Local proxy bind address
    #[serde(default = "default_proxy_host")]
    pub proxy_host: String,
    /// Auto-switch keys on 429/500
    #[serde(default = "default_true")]
    pub auto_switch: bool,
    /// List of configured platforms
    #[serde(default)]
    pub platforms: Vec<Platform>,
}

fn default_proxy_port() -> u16 {
    8080
}

fn default_proxy_host() -> String {
    "127.0.0.1".to_string()
}

fn default_true() -> bool {
    true
}

impl AppConfig {
    pub fn new() -> Self {
        Self {
            language: "zh".to_string(),
            theme: "system".to_string(),
            proxy_port: 8080,
            proxy_host: "127.0.0.1".to_string(),
            auto_switch: true,
            platforms: Vec::new(),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::new()
    }
}
