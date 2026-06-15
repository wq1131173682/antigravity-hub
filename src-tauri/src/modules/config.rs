use std::fs;
use serde_json;
use tracing::info;

use crate::models::AppConfig;
use super::platform_manager;

const CONFIG_FILE: &str = "gui_config.json";

/// Load application configuration
pub fn load_app_config() -> Result<AppConfig, String> {
    let data_dir = platform_manager::get_data_dir()?;
    let config_path = data_dir.join(CONFIG_FILE);

    if !config_path.exists() {
        let config = AppConfig::new();
        let _ = save_app_config(&config);
        return Ok(config);
    }

    let content = fs::read_to_string(&config_path)
        .map_err(|e| format!("failed_to_read_config_file: {}", e))?;

    let config: AppConfig = serde_json::from_str(&content)
        .map_err(|e| format!("failed_to_parse_config_file: {}", e))?;

    Ok(config)
}

/// Update proxy_port in config file AND the runtime static.
/// The runtime static is what start_proxy() actually reads.
pub fn set_proxy_port(port: u16) -> Result<(), String> {
    // Update runtime static immediately (so start_proxy always uses correct port)
    crate::modules::proxy::set_proxy_port_static(port);
    // Persist to config file
    let mut config = load_app_config()?;
    config.proxy_port = port;
    save_app_config(&config)
}

/// Update proxy_host in config file AND the runtime static.
pub fn set_proxy_host(host: String) -> Result<(), String> {
    // Update runtime static immediately
    crate::modules::proxy::set_proxy_host_static(host.clone());
    // Persist to config file
    let mut config = load_app_config()?;
    config.proxy_host = host;
    save_app_config(&config)
}

/// Save application configuration
pub fn save_app_config(config: &AppConfig) -> Result<(), String> {
    let data_dir = platform_manager::get_data_dir()?;
    let config_path = data_dir.join(CONFIG_FILE);

    // Write using temp file + atomic replace for safety
    let temp_path = data_dir.join("gui_config.json.tmp");
    let content = serde_json::to_string_pretty(config)
        .map_err(|e| format!("failed_to_serialize_config: {}", e))?;

    fs::write(&temp_path, &content)
        .map_err(|e| format!("failed_to_write_config: {}", e))?;

    fs::rename(&temp_path, &config_path)
        .map_err(|e| format!("failed_to_rename_config: {}", e))?;

    Ok(())
}
