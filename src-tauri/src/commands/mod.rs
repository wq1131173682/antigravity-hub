use crate::models::{AppConfig, Platform, ApiKey, Model};
use crate::modules::{self as modules};

// ============================================================================
// Platform Commands
// ============================================================================

/// List all platforms
#[tauri::command]
pub async fn list_platforms() -> Result<Vec<Platform>, String> {
    let config = modules::config::load_app_config()?;
    Ok(modules::platform_manager::list_platforms(&config))
}

/// Add a new platform
#[tauri::command]
pub async fn add_platform(name: String, base_url: String, path_prefix: String, notes: Option<String>) -> Result<Platform, String> {
    let mut config = modules::config::load_app_config()?;
    let platform = modules::platform_manager::add_platform(&mut config, name, base_url, path_prefix, notes);
    modules::config::save_app_config(&config)?;
    Ok(platform)
}

/// Update a platform
#[tauri::command]
pub async fn update_platform(
    platform_id: String,
    name: Option<String>,
    base_url: Option<String>,
    path_prefix: Option<String>,
    notes: Option<String>,
) -> Result<Platform, String> {
    let mut config = modules::config::load_app_config()?;
    let platform = modules::platform_manager::update_platform(&mut config, &platform_id, name, base_url, path_prefix, notes)?;
    modules::config::save_app_config(&config)?;
    Ok(platform)
}

/// Delete a platform and its keys, models, associations
#[tauri::command]
pub async fn delete_platform(platform_id: String) -> Result<(), String> {
    // Clean up models and model-key associations
    let _ = modules::key_model_map::remove_platform_associations(&platform_id);
    let _ = modules::model_manager::delete_platform_models(&platform_id);
    let _ = modules::quota_window::remove_platform_trackers(&platform_id);

    // Delete the platform from config
    let mut config = modules::config::load_app_config()?;
    modules::platform_manager::delete_platform(&mut config, &platform_id)?;
    modules::config::save_app_config(&config)
}

/// Reorder platforms
#[tauri::command]
pub async fn reorder_platforms(platform_ids: Vec<String>) -> Result<(), String> {
    let mut config = modules::config::load_app_config()?;
    modules::platform_manager::reorder_platforms(&mut config, platform_ids)?;
    modules::config::save_app_config(&config)
}

// ============================================================================
// Model Commands
// ============================================================================

/// List all models for a platform
#[tauri::command]
pub async fn list_models(platform_id: String) -> Result<Vec<Model>, String> {
    modules::model_manager::list_models(&platform_id)
}

/// Add a new model under a platform
#[tauri::command]
pub async fn add_model(
    platform_id: String,
    model_name: String,
    display_name: String,
    per_5hour: Option<u32>,
    per_day: Option<u32>,
    per_month: Option<u32>,
) -> Result<Model, String> {
    modules::model_manager::add_model(platform_id, model_name, display_name, per_5hour, per_day, per_month)
}

/// Update a model
#[tauri::command]
pub async fn update_model(
    model_id: String,
    model_name: Option<String>,
    display_name: Option<String>,
    per_5hour: Option<u32>,
    per_day: Option<u32>,
    per_month: Option<u32>,
) -> Result<Model, String> {
    modules::model_manager::update_model(&model_id, model_name, display_name, per_5hour, per_day, per_month)
}

/// Delete a model
#[tauri::command]
pub async fn delete_model(model_id: String) -> Result<(), String> {
    let _ = modules::key_model_map::remove_model_associations(&model_id);
    let _ = modules::quota_window::remove_model_trackers(&model_id);
    modules::model_manager::delete_model(&model_id)
}

// ============================================================================
// Key-Model Association Commands
// ============================================================================

/// Get all key IDs associated with a model
#[tauri::command]
pub async fn get_keys_for_model(model_id: String) -> Result<Vec<String>, String> {
    modules::key_model_map::get_keys_for_model(&model_id)
}

/// Get all model IDs associated with a key
#[tauri::command]
pub async fn get_models_for_key(key_id: String) -> Result<Vec<String>, String> {
    modules::key_model_map::get_models_for_key(&key_id)
}

/// Associate a key with a model
#[tauri::command]
pub async fn associate_key_with_model(key_id: String, model_id: String) -> Result<(), String> {
    modules::key_model_map::associate_key_with_model(&key_id, &model_id)
}

/// Disassociate a key from a model
#[tauri::command]
pub async fn disassociate_key_from_model(key_id: String, model_id: String) -> Result<(), String> {
    modules::key_model_map::disassociate_key_from_model(&key_id, &model_id)
}

// ============================================================================
// API Key Commands
// ============================================================================

/// List all keys for a platform
#[tauri::command]
pub async fn list_keys(platform_id: String) -> Result<Vec<ApiKey>, String> {
    modules::keystore::list_keys(&platform_id)
}

/// Add a new API key
#[tauri::command]
pub async fn add_key(platform_id: String, name: String, key_value: String) -> Result<ApiKey, String> {
    modules::keystore::add_key(platform_id, name, key_value)
}

/// Update an API key (name and/or value)
#[tauri::command]
pub async fn update_key(key_id: String, name: Option<String>, key_value: Option<String>) -> Result<ApiKey, String> {
    modules::keystore::update_key(&key_id, name, key_value)
}

/// Delete an API key
#[tauri::command]
pub async fn delete_key(key_id: String) -> Result<(), String> {
    let _ = modules::key_model_map::remove_key_associations(&key_id);
    let _ = modules::quota_window::remove_key_tracker(&key_id);
    modules::keystore::delete_key(&key_id)
}

/// Enable a disabled API key
#[tauri::command]
pub async fn enable_key(key_id: String) -> Result<ApiKey, String> {
    modules::keystore::set_key_status(&key_id, false, None, None)
}

/// Disable an API key
#[tauri::command]
pub async fn disable_key(key_id: String, reason: Option<String>) -> Result<ApiKey, String> {
    modules::keystore::set_key_status(&key_id, true, reason, None)
}

/// Set key status (enable/disable)
#[tauri::command]
pub async fn set_key_status(
    key_id: String,
    disabled: bool,
    disabled_reason: Option<String>,
    disabled_until: Option<i64>,
) -> Result<ApiKey, String> {
    modules::keystore::set_key_status(&key_id, disabled, disabled_reason, disabled_until)
}

// ============================================================================
// Quota Window Commands
// ============================================================================

/// Record an API call (with model tracking)
#[tauri::command]
pub async fn record_api_call_cmd(key_id: String, model_id: String, platform_id: String) -> Result<(), String> {
    modules::quota_window::record_api_call(&key_id, &model_id, &platform_id)
}

/// Record a 429 error (with model tracking)
#[tauri::command]
pub async fn record_429_error_cmd(key_id: String, model_id: String, platform_id: String) -> Result<(), String> {
    modules::quota_window::record_429_error(&key_id, &model_id, &platform_id)
}

/// Record a 500 error (with model tracking)
#[tauri::command]
pub async fn record_500_error_cmd(key_id: String, model_id: String, platform_id: String) -> Result<(), String> {
    modules::quota_window::record_500_error(&key_id, &model_id, &platform_id)
}

/// Get all quota window statuses
#[tauri::command]
pub async fn get_quota_window_status() -> Result<Vec<serde_json::Value>, String> {
    modules::quota_window::get_all_window_status()
}

/// Get usage summary for a key+model pair
#[tauri::command]
pub async fn get_key_usage(key_id: String, model_id: String) -> Result<serde_json::Value, String> {
    modules::quota_window::get_key_usage(&key_id, &model_id)
}

/// Get usage for all keys of a model
#[tauri::command]
pub async fn get_model_usage(model_id: String) -> Result<Vec<serde_json::Value>, String> {
    modules::quota_window::get_model_usage(&model_id)
}

/// Set auto-switch enabled/disabled
#[tauri::command]
pub async fn set_auto_switch_cmd(enabled: bool) -> Result<(), String> {
    modules::quota_window::set_auto_switch(enabled)
}

/// Get auto-switch status
#[tauri::command]
pub async fn get_auto_switch_cmd() -> Result<bool, String> {
    modules::quota_window::get_auto_switch_enabled()
}

/// Remove all quota trackers for a key
#[tauri::command]
pub async fn remove_quota_tracker(key_id: String) -> Result<(), String> {
    modules::quota_window::remove_key_tracker(&key_id)
}

/// Clean expired disabled keys
#[tauri::command]
pub async fn clean_expired_disabled_cmd() -> Result<Vec<String>, String> {
    modules::quota_window::clean_expired_disabled()
}

// ============================================================================
// Proxy Commands
// ============================================================================

/// Start the local proxy server (port is read from the runtime static,
/// which is updated by set_proxy_port)
#[tauri::command]
pub async fn start_proxy() -> Result<(), String> {
    modules::proxy::start_proxy().await
}

/// Stop the proxy server
#[tauri::command]
pub async fn stop_proxy() -> Result<(), String> {
    modules::proxy::stop_proxy()
}

/// Get proxy status
#[tauri::command]
pub async fn get_proxy_status() -> Result<modules::proxy::ProxyStatus, String> {
    Ok(modules::proxy::get_proxy_status())
}

// ============================================================================
// Config Commands
// ============================================================================

/// Load application config
#[tauri::command]
pub async fn load_config() -> Result<AppConfig, String> {
    modules::config::load_app_config()
}

/// Save application config
#[tauri::command]
pub async fn save_config(config: AppConfig) -> Result<(), String> {
    modules::config::save_app_config(&config)
}

/// Set proxy port only (dedicated command to avoid full AppConfig serialization).
/// Returns the saved port as verification.
#[tauri::command]
pub async fn set_proxy_port(port: u16) -> Result<u16, String> {
    modules::config::set_proxy_port(port)?;
    // Read back and return to verify
    let config = modules::config::load_app_config()?;
    Ok(config.proxy_port)
}

/// Set proxy host/bind address.
#[tauri::command]
pub async fn set_proxy_host(host: String) -> Result<(), String> {
    modules::config::set_proxy_host(host)
}

// ============================================================================
// Window Control Commands
// ============================================================================

/// Minimize the main window
#[tauri::command]
pub async fn minimize_window(window: tauri::Window) -> Result<(), String> {
    window.minimize().map_err(|e| e.to_string())
}

/// Toggle maximize/restore the main window
#[tauri::command]
pub async fn toggle_maximize_window(window: tauri::Window) -> Result<(), String> {
    if window.is_maximized().unwrap_or(false) {
        window.unmaximize().map_err(|e| e.to_string())
    } else {
        window.maximize().map_err(|e| e.to_string())
    }
}

/// Close/hide the main window (minimize to tray)
#[tauri::command]
pub async fn close_window(window: tauri::Window) -> Result<(), String> {
    window.hide().map_err(|e| e.to_string())
}

// ============================================================================
// Utility Commands
// ============================================================================

fn validate_path(path: &str) -> Result<(), String> {
    if path.contains("..") {
        return Err("非法路径: 不允许目录遍历".to_string());
    }
    let lower_path = path.to_lowercase();
    let sensitive_prefixes = [
        "/etc/", "/var/spool/cron", "/root/", "/proc/", "/sys/", "/dev/",
        "c:\\windows", "c:\\users\\administrator", "c:\\pagefile.sys",
    ];
    for prefix in sensitive_prefixes {
        if lower_path.starts_with(prefix) {
            return Err(format!("安全拒绝: 禁止访问系统敏感路径 ({})", prefix));
        }
    }
    Ok(())
}

/// Save text file
#[tauri::command]
pub async fn save_text_file(path: String, content: String) -> Result<(), String> {
    validate_path(&path)?;
    std::fs::write(&path, content).map_err(|e| format!("写入文件失败: {}", e))
}

/// Read text file
#[tauri::command]
pub async fn read_text_file(path: String) -> Result<String, String> {
    validate_path(&path)?;
    std::fs::read_to_string(&path).map_err(|e| format!("读取文件失败: {}", e))
}

/// Clear log cache
#[tauri::command]
pub async fn clear_log_cache() -> Result<(), String> {
    modules::logger::clear_logs()
}

/// Open data folder
#[tauri::command]
pub async fn open_data_folder() -> Result<(), String> {
    let path = modules::platform_manager::get_data_dir()?;
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(&path).spawn()
            .map_err(|e| format!("打开文件夹失败: {}", e))?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer").arg(&path).spawn()
            .map_err(|e| format!("打开文件夹失败: {}", e))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(&path).spawn()
            .map_err(|e| format!("打开文件夹失败: {}", e))?;
    }
    Ok(())
}

/// Get data directory absolute path
#[tauri::command]
pub async fn get_data_dir_path() -> Result<String, String> {
    let path = modules::platform_manager::get_data_dir()?;
    Ok(path.to_string_lossy().to_string())
}

/// Show main window
#[tauri::command]
pub async fn show_main_window(window: tauri::Window) -> Result<(), String> {
    window.show().map_err(|e| e.to_string())
}

/// Set window theme
#[tauri::command]
pub async fn set_window_theme(window: tauri::Window, theme: String) -> Result<(), String> {
    use tauri::Theme;
    let tauri_theme = match theme.as_str() {
        "dark" => Some(Theme::Dark),
        "light" => Some(Theme::Light),
        _ => None,
    };
    window.set_theme(tauri_theme).map_err(|e| e.to_string())
}

/// Get the primary LAN IP address (for displaying when proxy binds to 0.0.0.0)
#[tauri::command]
pub fn get_lan_ip() -> Result<String, String> {
    // UDP trick: connect to an external address to learn which local IP
    // is used for outbound traffic. No packets are actually sent.
    let socket = std::net::UdpSocket::bind("0.0.0.0:0")
        .map_err(|e| format!("Failed to bind: {}", e))?;
    socket.connect("8.8.8.8:80")
        .map_err(|e| format!("Failed to connect: {}", e))?;
    let local_addr = socket.local_addr()
        .map_err(|e| format!("Failed to get local addr: {}", e))?;
    Ok(local_addr.ip().to_string())
}
