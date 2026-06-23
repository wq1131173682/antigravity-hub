use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;
use crate::models::{ApiKey, KeyStatus};

use super::platform_manager;

const KEY_STORE_FILE: &str = "api_keys.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyStore {
    pub keys: Vec<ApiKey>,
    /// Round-robin index per platform (platform_id -> next index)
    #[serde(default)]
    pub rotation_index: std::collections::HashMap<String, usize>,
}

impl KeyStore {
    pub fn new() -> Self {
        Self {
            keys: Vec::new(),
            rotation_index: std::collections::HashMap::new(),
        }
    }
}

// ── Key store persistence ──

pub fn load_key_store() -> Result<KeyStore, String> {
    let path = get_keys_file_path()?;
    if !path.exists() {
        return Ok(KeyStore::new());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("read api_keys failed: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("parse api_keys failed: {}", e))
}

pub fn save_key_store(store: &KeyStore) -> Result<(), String> {
    let path = get_keys_file_path()?;
    let content = serde_json::to_string_pretty(store)
        .map_err(|e| format!("serialize api_keys failed: {}", e))?;
    fs::write(&path, content).map_err(|e| format!("write api_keys failed: {}", e))
}

fn get_keys_file_path() -> Result<PathBuf, String> {
    let data_dir = platform_manager::get_data_dir()?;
    Ok(data_dir.join(KEY_STORE_FILE))
}

// ── Key CRUD ──

/// List all keys for a platform
pub fn list_keys(platform_id: &str) -> Result<Vec<ApiKey>, String> {
    let store = load_key_store()?;
    let mut keys: Vec<ApiKey> = store.keys.into_iter()
        .filter(|k| k.platform_id == platform_id)
        .collect();
    keys.sort_by_key(|k| k.sort_order);
    Ok(keys)
}

/// List all keys across all platforms
pub fn list_all_keys() -> Result<Vec<ApiKey>, String> {
    let store = load_key_store()?;
    Ok(store.keys)
}

/// Add a new API key
pub fn add_key(platform_id: String, name: String, key_value: String) -> Result<ApiKey, String> {
    let mut store = load_key_store()?;
    let id = Uuid::new_v4().to_string();
    let mut key = ApiKey::new(id, platform_id, name, key_value);
    key.sort_order = store.keys.len() as i32;
    store.keys.push(key.clone());
    save_key_store(&store)?;
    Ok(key)
}

/// Update an API key
pub fn update_key(
    key_id: &str,
    name: Option<String>,
    key_value: Option<String>,
) -> Result<ApiKey, String> {
    let mut store = load_key_store()?;
    let key = store.keys.iter_mut()
        .find(|k| k.id == key_id)
        .ok_or_else(|| format!("Key not found: {}", key_id))?;
    
    if let Some(name) = name {
        key.name = name;
    }
    if let Some(key_value) = key_value {
        key.key_value = key_value;
    }
    
    let result = key.clone();
    save_key_store(&store)?;
    Ok(result)
}

/// Delete an API key
pub fn delete_key(key_id: &str) -> Result<(), String> {
    let mut store = load_key_store()?;
    let pos = store.keys.iter().position(|k| k.id == key_id)
        .ok_or_else(|| format!("Key not found: {}", key_id))?;
    store.keys.remove(pos);
    save_key_store(&store)
}

/// Set key status (enable/disable)
pub fn set_key_status(key_id: &str, disabled: bool, reason: Option<String>, disabled_until: Option<i64>) -> Result<ApiKey, String> {
    let mut store = load_key_store()?;
    let key = store.keys.iter_mut()
        .find(|k| k.id == key_id)
        .ok_or_else(|| format!("Key not found: {}", key_id))?;
    
    if disabled {
        key.status = KeyStatus::Disabled;
        key.disabled_reason = reason;
        key.disabled_until = disabled_until;
    } else {
        key.status = KeyStatus::Active;
        key.disabled_reason = None;
        key.disabled_until = None;
    }
    
    let result = key.clone();
    save_key_store(&store)?;
    Ok(result)
}

// ── Key rotation ──

/// Get the next active key for a platform (round-robin)
pub fn get_next_active_key(platform_id: &str) -> Result<Option<ApiKey>, String> {
    let store = load_key_store()?;
    let len = store.keys.iter()
        .filter(|k| k.platform_id == platform_id && k.is_active())
        .count();
    
    if len == 0 {
        return Ok(None);
    }
    
    let idx = store.rotation_index.get(platform_id).copied().unwrap_or(0);
    let next_idx = idx % len;
    
    // Get the key at the rotation index
    let key: Option<ApiKey> = store.keys.iter()
        .filter(|k| k.platform_id == platform_id && k.is_active())
        .nth(next_idx)
        .cloned();
    
    // Update rotation index
    let mut store = load_key_store()?;
    store.rotation_index.insert(platform_id.to_string(), (idx + 1) % len);
    save_key_store(&store)?;
    
    Ok(key)
}

/// Get the best available key for a platform (lowest usage first, considering quota)
/// This combines key availability with quota window data
pub fn get_best_available_key(platform_id: &str) -> Result<Option<ApiKey>, String> {
    let store = load_key_store()?;
    let api_keys: Vec<ApiKey> = store.keys.iter()
        .filter(|k| k.platform_id == platform_id && k.is_active())
        .cloned()
        .collect();
    
    if api_keys.is_empty() {
        return Ok(None);
    }
    
    // Check quota windows - prefer keys with lowest usage
    let quota_state = super::quota_window::load_quota_state_internal().ok();
    let mut scored_keys: Vec<(ApiKey, u32)> = api_keys.into_iter().map(|k| {
        let total_usage = quota_state.as_ref()
            .and_then(|state| state.trackers.iter().find(|t| t.key_id == k.id))
            .map(|t| t.five_hour.len() + t.day.len() + t.month.len())
            .unwrap_or(0);
        (k, total_usage)
    }).collect();
    
    scored_keys.sort_by_key(|(_, score)| *score);
    
    Ok(scored_keys.into_iter().next().map(|(k, _)| k))
}

/// Count active keys for a platform
pub fn count_active_keys(platform_id: &str) -> Result<usize, String> {
    let store = load_key_store()?;
    Ok(store.keys.iter()
        .filter(|k| k.platform_id == platform_id && k.is_active())
        .count())
}

/// Reorder keys
pub fn reorder_keys(key_ids: Vec<String>) -> Result<(), String> {
    let mut store = load_key_store()?;
    let mut reordered = Vec::new();
    for (i, id) in key_ids.iter().enumerate() {
        if let Some(key) = store.keys.iter().find(|k| &k.id == id) {
            let mut k = key.clone();
            k.sort_order = i as i32;
            reordered.push(k);
        }
    }
    // Add any keys not in the list
    for k in &store.keys {
        if !key_ids.contains(&k.id) {
            let mut k = k.clone();
            k.sort_order = reordered.len() as i32;
            reordered.push(k);
        }
    }
    store.keys = reordered;
    save_key_store(&store)
}
