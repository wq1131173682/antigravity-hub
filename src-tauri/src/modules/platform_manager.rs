use std::fs;
use std::path::PathBuf;
use uuid::Uuid;
use crate::models::Platform;

// ── Platform CRUD ──

/// List all platforms (from AppConfig)
pub fn list_platforms(config: &crate::models::AppConfig) -> Vec<Platform> {
    let mut platforms = config.platforms.clone();
    platforms.sort_by_key(|p| p.sort_order);
    platforms
}

/// Add a new platform
pub fn add_platform(
    config: &mut crate::models::AppConfig,
    name: String,
    base_url: String,
    path_prefix: String,
    notes: Option<String>,
) -> Platform {
    let id = Uuid::new_v4().to_string();
    let mut platform = Platform::new(id, name, base_url, path_prefix);
    platform.notes = notes;
    platform.sort_order = config.platforms.len() as i32;
    config.platforms.push(platform.clone());
    platform
}

/// Update a platform
pub fn update_platform(
    config: &mut crate::models::AppConfig,
    platform_id: &str,
    name: Option<String>,
    base_url: Option<String>,
    path_prefix: Option<String>,
    notes: Option<String>,
) -> Result<Platform, String> {
    let platform = config.platforms.iter_mut()
        .find(|p| p.id == platform_id)
        .ok_or_else(|| format!("Platform not found: {}", platform_id))?;
    
    if let Some(name) = name {
        platform.name = name;
    }
    if let Some(base_url) = base_url {
        platform.base_url = base_url;
    }
    if let Some(path_prefix) = path_prefix {
        platform.path_prefix = path_prefix;
    }
    platform.notes = notes;
    
    Ok(platform.clone())
}

/// Delete a platform and its keys
pub fn delete_platform(
    config: &mut crate::models::AppConfig,
    platform_id: &str,
) -> Result<(), String> {
    let pos = config.platforms.iter().position(|p| p.id == platform_id)
        .ok_or_else(|| format!("Platform not found: {}", platform_id))?;
    config.platforms.remove(pos);
    
    // Also delete all API keys for this platform
    if let Ok(mut key_store) = super::keystore::load_key_store() {
        key_store.keys.retain(|k| k.platform_id != platform_id);
        let _ = super::keystore::save_key_store(&key_store);
    }
    
    Ok(())
}

/// Reorder platforms
pub fn reorder_platforms(
    config: &mut crate::models::AppConfig,
    platform_ids: Vec<String>,
) -> Result<(), String> {
    let mut reordered = Vec::new();
    for (i, id) in platform_ids.iter().enumerate() {
        if let Some(platform) = config.platforms.iter().find(|p| &p.id == id) {
            let mut p = platform.clone();
            p.sort_order = i as i32;
            reordered.push(p);
        }
    }
    // Add any platforms not in the list
    for p in &config.platforms {
        if !platform_ids.contains(&p.id) {
            let mut p = p.clone();
            p.sort_order = reordered.len() as i32;
            reordered.push(p);
        }
    }
    config.platforms = reordered;
    Ok(())
}

/// Get data directory
pub fn get_data_dir() -> Result<PathBuf, String> {
    if let Ok(env_path) = std::env::var("ABV_DATA_DIR") {
        if !env_path.trim().is_empty() {
            let data_dir = PathBuf::from(env_path);
            if !data_dir.exists() {
                fs::create_dir_all(&data_dir)
                    .map_err(|e| format!("failed_to_create_custom_data_dir: {}", e))?;
            }
            return Ok(data_dir);
        }
    }

    let home = dirs::home_dir().ok_or("failed_to_get_home_dir")?;
    let data_dir = home.join(".antigravity_tools");
    if !data_dir.exists() {
        fs::create_dir_all(&data_dir).map_err(|e| format!("failed_to_create_data_dir: {}", e))?;
    }
    Ok(data_dir)
}
