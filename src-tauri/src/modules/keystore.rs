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

// 鈹€鈹€ Key store persistence 鈹€鈹€

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

// 鈹€鈹€ Key CRUD 鈹€鈹€

/// List all keys for a platform
pub fn list_keys(platform_id: &str) -> Result<Vec<ApiKey>, String> {
    let store = load_key_store()?;
    let mut keys: Vec<ApiKey> = store.keys.into_iter()
        .filter(|k| k.platform_id == platform_id)
        .collect();
    keys.sort_by_key(|k| k.sort_order);
    Ok(keys)
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

